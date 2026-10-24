//! Trident — a three-oscillator subtractive synthesizer.
//!
//! Fills the musical role of a classic "three oscillators into a filter" starter
//! synth: per-oscillator waveform, coarse/fine tuning and level, one resonant
//! low-pass filter, and an amplitude ADSR. Original design and code.

use soundesti_dsp::{Adsr, AdsrParams, Oscillator, Svf, SvfMode, Waveform, midi_to_freq};
use soundesti_plugin_api::{Instrument, NoteEvent, NoteId, ParamInfo, ParamKind, TimedEvent, for_each_segment};

pub const TYPE_ID: &str = "soundesti.trident";
pub const MAX_VOICES: usize = 16;
const OSCS: usize = 3;

const WAVE_LABELS: &[&str] = &["Sine", "Triangle", "Saw", "Square", "Noise"];

pub mod param {
    /// Oscillator `n` (0..3) parameters live at `n * 10 + offset`.
    pub const OSC_WAVE: u32 = 0;
    pub const OSC_COARSE: u32 = 1;
    pub const OSC_FINE: u32 = 2;
    pub const OSC_LEVEL: u32 = 3;
    pub const fn osc(n: u32, offset: u32) -> u32 {
        n * 10 + offset
    }
    pub const ATTACK: u32 = 100;
    pub const DECAY: u32 = 101;
    pub const SUSTAIN: u32 = 102;
    pub const RELEASE: u32 = 103;
    pub const CUTOFF: u32 = 104;
    pub const RESONANCE: u32 = 105;
    pub const VOLUME: u32 = 106;
    pub const VELOCITY_SENS: u32 = 107;
}

macro_rules! osc_params {
    ($n:literal, $wave:literal, $coarse:literal, $level:literal) => {
        [
            ParamInfo {
                id: param::osc($n, param::OSC_WAVE),
                key: concat!("osc", $n, ".wave"),
                name: "Shape",
                group: concat!("Osc ", $n),
                min: 0.0,
                max: 4.0,
                default: $wave,
                unit: "",
                kind: ParamKind::Stepped { labels: WAVE_LABELS },
            },
            ParamInfo {
                id: param::osc($n, param::OSC_COARSE),
                key: concat!("osc", $n, ".coarse"),
                name: "Coarse",
                group: concat!("Osc ", $n),
                min: -24.0,
                max: 24.0,
                default: $coarse,
                unit: "st",
                kind: ParamKind::Stepped { labels: &[] },
            },
            ParamInfo {
                id: param::osc($n, param::OSC_FINE),
                key: concat!("osc", $n, ".fine"),
                name: "Fine",
                group: concat!("Osc ", $n),
                min: -100.0,
                max: 100.0,
                default: 0.0,
                unit: "ct",
                kind: ParamKind::Continuous,
            },
            ParamInfo {
                id: param::osc($n, param::OSC_LEVEL),
                key: concat!("osc", $n, ".level"),
                name: "Level",
                group: concat!("Osc ", $n),
                min: 0.0,
                max: 1.0,
                default: $level,
                unit: "",
                kind: ParamKind::Continuous,
            },
        ]
    };
}

#[allow(clippy::too_many_arguments)]
const fn cont(
    id: u32,
    key: &'static str,
    name: &'static str,
    group: &'static str,
    min: f32,
    max: f32,
    default: f32,
    unit: &'static str,
) -> ParamInfo {
    ParamInfo { id, key, name, group, min, max, default, unit, kind: ParamKind::Continuous }
}

const OSC0: [ParamInfo; 4] = osc_params!(0, 2.0, 0.0, 0.8);
const OSC1: [ParamInfo; 4] = osc_params!(1, 3.0, -12.0, 0.4);
const OSC2: [ParamInfo; 4] = osc_params!(2, 0.0, -24.0, 0.3);

pub static PARAMS: [ParamInfo; 20] = [
    OSC0[0],
    OSC0[1],
    OSC0[2],
    OSC0[3],
    OSC1[0],
    OSC1[1],
    OSC1[2],
    OSC1[3],
    OSC2[0],
    OSC2[1],
    OSC2[2],
    OSC2[3],
    cont(param::ATTACK, "amp.attack", "Attack", "Envelope", 0.0, 5.0, 0.003, "s"),
    cont(param::DECAY, "amp.decay", "Decay", "Envelope", 0.0, 5.0, 0.35, "s"),
    cont(param::SUSTAIN, "amp.sustain", "Sustain", "Envelope", 0.0, 1.0, 0.6, ""),
    cont(param::RELEASE, "amp.release", "Release", "Envelope", 0.0, 5.0, 0.12, "s"),
    cont(param::CUTOFF, "filter.cutoff", "Cutoff", "Filter", 20.0, 20_000.0, 6_000.0, "Hz"),
    cont(param::RESONANCE, "filter.resonance", "Resonance", "Filter", 0.5, 12.0, 0.9, "Q"),
    cont(param::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.8, ""),
    cont(param::VELOCITY_SENS, "master.velocity", "Velocity sens.", "Master", 0.0, 1.0, 0.7, ""),
];

#[derive(Clone, Copy, Debug)]
struct OscSettings {
    wave: Waveform,
    coarse: f32,
    fine: f32,
    level: f32,
}

struct Voice {
    oscs: [Oscillator; OSCS],
    env: Adsr,
    filter: Svf,
    note_id: NoteId,
    key: u8,
    gain: f32,
    /// Allocation order; lower = older (used for stealing).
    age: u64,
    gate: bool,
}

pub struct Trident {
    sample_rate: f32,
    values: [f32; PARAMS.len()],
    osc: [OscSettings; OSCS],
    adsr: AdsrParams,
    cutoff: f32,
    resonance: f32,
    volume: f32,
    velocity_sens: f32,
    voices: Vec<Voice>,
    next_age: u64,
}

impl Trident {
    pub fn new(sample_rate: f32) -> Self {
        let voices = (0..MAX_VOICES)
            .map(|i| Voice {
                oscs: std::array::from_fn(|o| Oscillator::new(0x9E37_79B9 ^ (i * 3 + o + 1) as u32)),
                env: Adsr::new(sample_rate),
                filter: Svf::default(),
                note_id: 0,
                key: 60,
                gain: 0.0,
                age: 0,
                gate: false,
            })
            .collect();
        let mut synth = Self {
            sample_rate,
            values: [0.0; PARAMS.len()],
            osc: [OscSettings { wave: Waveform::Saw, coarse: 0.0, fine: 0.0, level: 0.0 }; OSCS],
            adsr: AdsrParams::default(),
            cutoff: 6_000.0,
            resonance: 0.9,
            volume: 0.8,
            velocity_sens: 0.7,
            voices,
            next_age: 1,
        };
        for p in &PARAMS {
            synth.set_param(p.id, p.default);
        }
        synth
    }

    fn index_of(id: u32) -> Option<usize> {
        PARAMS.iter().position(|p| p.id == id)
    }

    fn voice_freq(&self, key: u8, osc: usize) -> f32 {
        let s = self.osc[osc];
        midi_to_freq(key as f32 + s.coarse + s.fine / 100.0)
    }

    fn start_voice(&mut self, id: NoteId, key: u8, velocity: f32) {
        // Prefer an idle voice; otherwise steal the oldest. Gate-on ramps from the
        // stolen voice's current level, so stealing does not click.
        let idx = self
            .voices
            .iter()
            .position(|v| v.env.is_idle())
            .or_else(|| self.voices.iter().enumerate().min_by_key(|(_, v)| (v.gate, v.age)).map(|(i, _)| i))
            .unwrap_or(0);
        let freqs: [f32; OSCS] = std::array::from_fn(|o| self.voice_freq(key, o));
        let vel = velocity.clamp(0.0, 1.0);
        let gain = 1.0 - self.velocity_sens + self.velocity_sens * vel * vel;
        let age = self.next_age;
        self.next_age += 1;
        let sr = self.sample_rate;
        let (cutoff, res, adsr) = (self.cutoff, self.resonance, self.adsr);
        let v = &mut self.voices[idx];
        if v.env.is_idle() {
            v.filter.reset();
            for o in &mut v.oscs {
                o.set_phase(0.0);
            }
        }
        for (o, f) in v.oscs.iter_mut().zip(freqs) {
            o.set_frequency(f, sr);
        }
        v.filter.set(cutoff, res, sr);
        v.env.set_params(adsr);
        v.env.gate_on();
        v.note_id = id;
        v.key = key;
        v.gain = gain;
        v.age = age;
        v.gate = true;
    }

    fn handle(&mut self, ev: NoteEvent) {
        match ev {
            NoteEvent::On { id, key, velocity } => self.start_voice(id, key, velocity),
            NoteEvent::Off { id } => {
                for v in self.voices.iter_mut().filter(|v| v.gate && v.note_id == id) {
                    v.gate = false;
                    v.env.gate_off();
                }
            }
            NoteEvent::AllNotesOff => {
                for v in &mut self.voices {
                    v.gate = false;
                    v.env.gate_off();
                }
            }
            NoteEvent::AllSoundOff => {
                for v in &mut self.voices {
                    v.gate = false;
                    v.env.kill();
                }
            }
        }
    }

    fn render(&mut self, range: std::ops::Range<usize>, left: &mut [f32], right: &mut [f32]) {
        let osc = self.osc;
        let out_gain = self.volume * 0.3;
        for v in self.voices.iter_mut().filter(|v| !v.env.is_idle()) {
            for i in range.clone() {
                let mut s = 0.0;
                for (o, set) in v.oscs.iter_mut().zip(osc.iter()) {
                    s += o.next(set.wave) * set.level;
                }
                let y = v.filter.process(s, SvfMode::LowPass) * v.env.next() * v.gain * out_gain;
                left[i] += y;
                right[i] += y;
                if v.env.is_idle() {
                    break;
                }
            }
        }
    }

    /// Re-applies live parameter changes to sounding voices.
    fn refresh_voices(&mut self) {
        let sr = self.sample_rate;
        for vi in 0..self.voices.len() {
            if self.voices[vi].env.is_idle() {
                continue;
            }
            let key = self.voices[vi].key;
            let freqs: [f32; OSCS] = std::array::from_fn(|o| self.voice_freq(key, o));
            let v = &mut self.voices[vi];
            for (o, f) in v.oscs.iter_mut().zip(freqs) {
                o.set_frequency(f, sr);
            }
            v.filter.set(self.cutoff, self.resonance, sr);
            v.env.set_params(self.adsr);
        }
    }
}

impl Instrument for Trident {
    fn type_id(&self) -> &'static str {
        TYPE_ID
    }

    fn display_name(&self) -> &'static str {
        "Trident"
    }

    fn params(&self) -> &'static [ParamInfo] {
        &PARAMS
    }

    fn get_param(&self, id: u32) -> f32 {
        Self::index_of(id).map(|i| self.values[i]).unwrap_or(0.0)
    }

    fn set_param(&mut self, id: u32, value: f32) {
        let Some(i) = Self::index_of(id) else { return };
        let info = PARAMS[i];
        let value = if value.is_finite() { value.clamp(info.min, info.max) } else { info.default };
        self.values[i] = value;
        match id {
            param::ATTACK => self.adsr.attack = value,
            param::DECAY => self.adsr.decay = value,
            param::SUSTAIN => self.adsr.sustain = value,
            param::RELEASE => self.adsr.release = value,
            param::CUTOFF => self.cutoff = value,
            param::RESONANCE => self.resonance = value,
            param::VOLUME => self.volume = value,
            param::VELOCITY_SENS => self.velocity_sens = value,
            _ => {
                let (n, off) = ((id / 10) as usize, id % 10);
                if n < OSCS {
                    let o = &mut self.osc[n];
                    match off {
                        param::OSC_WAVE => o.wave = Waveform::from_param(value),
                        param::OSC_COARSE => o.coarse = value.round(),
                        param::OSC_FINE => o.fine = value,
                        param::OSC_LEVEL => o.level = value,
                        _ => {}
                    }
                }
            }
        }
        self.refresh_voices();
    }

    fn process(&mut self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]) {
        let frames = left.len().min(right.len());
        // Split borrows: the helper needs `self` for state and the buffers for output.
        let mut ctx = (self, left, right);
        for_each_segment(
            &mut ctx,
            events,
            frames,
            |(s, _, _), ev| s.handle(ev),
            |(s, l, r), range| s.render(range, l, r),
        );
    }

    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| !v.env.is_idle()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn on(offset: u32, id: NoteId, key: u8) -> TimedEvent {
        TimedEvent { offset, event: NoteEvent::On { id, key, velocity: 0.8 } }
    }

    fn off(offset: u32, id: NoteId) -> TimedEvent {
        TimedEvent { offset, event: NoteEvent::Off { id } }
    }

    fn block(s: &mut Trident, events: &[TimedEvent], n: usize) -> (Vec<f32>, Vec<f32>) {
        let mut l = vec![0.0; n];
        let mut r = vec![0.0; n];
        s.process(events, &mut l, &mut r);
        (l, r)
    }

    fn rms(v: &[f32]) -> f32 {
        (v.iter().map(|x| x * x).sum::<f32>() / v.len().max(1) as f32).sqrt()
    }

    #[test]
    fn silent_without_notes() {
        let mut s = Trident::new(SR);
        let (l, _) = block(&mut s, &[], 512);
        assert!(l.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn note_on_produces_sound_starting_at_offset() {
        let mut s = Trident::new(SR);
        let (l, r) = block(&mut s, &[on(100, 1, 60)], 1024);
        assert!(l[..100].iter().all(|&x| x == 0.0), "sound before note offset");
        assert!(rms(&l[100..]) > 0.01);
        assert_eq!(l, r, "mono voice should be centred");
        assert_eq!(s.active_voices(), 1);
    }

    #[test]
    fn note_off_releases_to_silence() {
        let mut s = Trident::new(SR);
        block(&mut s, &[on(0, 1, 60)], 4800);
        block(&mut s, &[off(0, 1)], 4800);
        // Default release 0.12 s = 5760 samples; give it a full second.
        block(&mut s, &[], 48_000);
        assert_eq!(s.active_voices(), 0);
        let (l, _) = block(&mut s, &[], 256);
        assert!(l.iter().all(|&x| x == 0.0));
    }

    #[test]
    fn overlapping_same_key_notes_are_independent() {
        let mut s = Trident::new(SR);
        block(&mut s, &[on(0, 1, 60), on(10, 2, 60)], 256);
        assert_eq!(s.active_voices(), 2);
        block(&mut s, &[off(0, 1)], 48_000);
        assert_eq!(s.active_voices(), 1, "note 2 must keep sounding");
    }

    #[test]
    fn voice_stealing_caps_polyphony() {
        let mut s = Trident::new(SR);
        let events: Vec<TimedEvent> = (0..40).map(|i| on(i, i, 40 + i as u8)).collect();
        let (l, _) = block(&mut s, &events, 1024);
        assert_eq!(s.active_voices(), MAX_VOICES);
        assert!(l.iter().all(|x| x.is_finite()));
    }

    #[test]
    fn panic_silences_quickly() {
        let mut s = Trident::new(SR);
        block(&mut s, &[on(0, 1, 60), on(0, 2, 64)], 2048);
        block(&mut s, &[TimedEvent { offset: 0, event: NoteEvent::AllSoundOff }], 480);
        assert_eq!(s.active_voices(), 0, "kill fade is 2 ms = 96 samples");
    }

    #[test]
    fn params_clamp_and_round_trip() {
        let mut s = Trident::new(SR);
        s.set_param(param::CUTOFF, 1e9);
        assert_eq!(s.get_param(param::CUTOFF), 20_000.0);
        s.set_param(param::CUTOFF, f32::NAN);
        assert_eq!(s.get_param(param::CUTOFF), 6_000.0);
        s.set_param(param::osc(1, param::OSC_WAVE), 3.2);
        assert_eq!(Waveform::from_param(s.get_param(param::osc(1, param::OSC_WAVE))), Waveform::Square);
        let ids: std::collections::HashSet<u32> = PARAMS.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), PARAMS.len(), "duplicate param id");
    }

    #[test]
    fn higher_key_has_higher_pitch() {
        // Pure sine on osc 0 only, filter open, compare zero crossings.
        let crossings = |key: u8| {
            let mut s = Trident::new(SR);
            s.set_param(param::osc(0, param::OSC_WAVE), 0.0);
            s.set_param(param::osc(1, param::OSC_LEVEL), 0.0);
            s.set_param(param::osc(2, param::OSC_LEVEL), 0.0);
            s.set_param(param::CUTOFF, 20_000.0);
            s.set_param(param::SUSTAIN, 1.0);
            let (l, _) = block(&mut s, &[on(0, 1, key)], 48_000);
            l.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as i64
        };
        let a4 = crossings(69);
        assert!((a4 - 440).abs() <= 2, "A4 crossings {a4}");
        let a5 = crossings(81);
        assert!((a5 - 880).abs() <= 2, "A5 crossings {a5}");
    }
}
