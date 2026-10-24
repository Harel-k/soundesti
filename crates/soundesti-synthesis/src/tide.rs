//! Tide — wavetable synthesizer (the role of a wavetable synth). Eight single-cycle
//! frames from sine to bright pulse are morphed by Position (with its own LFO); each
//! frame is stored band-limited per octave (additive synthesis), so high notes don't
//! alias. Unison up to 5 detuned oscillators per voice, low-pass filter, ADSR.
//! Original design and code.

use std::sync::OnceLock;

use soundesti_dsp::{Adsr, AdsrParams, Svf, SvfMode};
use soundesti_plugin_api::{Instrument, NoteEvent, NoteId, ParamInfo, TimedEvent, for_each_segment};

use crate::common::{ParamStore, cont, pick_voice, stepped};

pub const TYPE_ID: &str = "soundesti.tide";
const VOICES: usize = 16;
const TABLE: usize = 2048;
const FRAMES: usize = 8;
/// Mip levels: level `m` keeps harmonics up to 512 >> m.
const LEVELS: usize = 9;
const MAX_UNISON: usize = 5;

pub mod param {
    pub const POSITION: u32 = 0;
    pub const LFO_RATE: u32 = 1;
    pub const LFO_AMOUNT: u32 = 2;
    pub const UNISON: u32 = 3;
    pub const DETUNE: u32 = 4;
    pub const CUTOFF: u32 = 5;
    pub const RESONANCE: u32 = 6;
    pub const ATTACK: u32 = 7;
    pub const DECAY: u32 = 8;
    pub const SUSTAIN: u32 = 9;
    pub const RELEASE: u32 = 10;
    pub const VOLUME: u32 = 11;
}

pub static PARAMS: [ParamInfo; 12] = [
    cont(param::POSITION, "wave.position", "Position", "Wavetable", 0.0, 1.0, 0.3, ""),
    cont(param::LFO_RATE, "wave.lfo_rate", "LFO rate", "Wavetable", 0.01, 10.0, 0.5, "Hz"),
    cont(param::LFO_AMOUNT, "wave.lfo_amount", "LFO amount", "Wavetable", 0.0, 1.0, 0.0, ""),
    stepped(param::UNISON, "unison.voices", "Unison", "Unison", 1.0, 5.0, 1.0, &[]),
    cont(param::DETUNE, "unison.detune", "Detune", "Unison", 0.0, 50.0, 12.0, "ct"),
    cont(param::CUTOFF, "filter.cutoff", "Cutoff", "Filter", 40.0, 20_000.0, 12_000.0, "Hz"),
    cont(param::RESONANCE, "filter.resonance", "Resonance", "Filter", 0.5, 10.0, 0.8, "Q"),
    cont(param::ATTACK, "amp.attack", "Attack", "Envelope", 0.0, 5.0, 0.005, "s"),
    cont(param::DECAY, "amp.decay", "Decay", "Envelope", 0.0, 5.0, 0.3, "s"),
    cont(param::SUSTAIN, "amp.sustain", "Sustain", "Envelope", 0.0, 1.0, 0.7, ""),
    cont(param::RELEASE, "amp.release", "Release", "Envelope", 0.0, 5.0, 0.25, "s"),
    cont(param::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.6, ""),
];

/// Harmonic amplitude of frame `f` (0 = sine .. 7 = narrow pulse) for harmonic `h` (1-based).
fn harmonic(f: usize, h: usize) -> f32 {
    let hf = h as f32;
    let odd = h % 2 == 1;
    match f {
        0 => (h == 1) as u8 as f32, // sine
        1 => {
            if h <= 3 {
                [1.0, 0.0, 0.25][h - 1]
            } else {
                0.0
            }
        } // sine + third
        2 => {
            if odd {
                (if (h / 2).is_multiple_of(2) { 1.0 } else { -1.0 }) / (hf * hf)
            } else {
                0.0
            }
        } // triangle
        3 => 1.0 / hf * (1.0 - hf / 600.0).max(0.0), // soft saw
        4 => 1.0 / hf,              // saw
        5 => {
            if odd {
                1.0 / hf
            } else {
                0.0
            }
        } // square
        6 => (std::f32::consts::PI * hf * 0.25).sin() / hf, // 25 % pulse
        _ => (std::f32::consts::PI * hf * 0.1).sin() / hf * 1.4, // 10 % pulse
    }
}

/// tables[level][frame][sample], normalized per frame.
fn tables() -> &'static Vec<Vec<Vec<f32>>> {
    static T: OnceLock<Vec<Vec<Vec<f32>>>> = OnceLock::new();
    T.get_or_init(|| {
        (0..LEVELS)
            .map(|level| {
                let max_h = (512usize >> level).max(1);
                (0..FRAMES)
                    .map(|f| {
                        let mut t = vec![0.0f32; TABLE];
                        for h in 1..=max_h {
                            let a = harmonic(f, h);
                            if a == 0.0 {
                                continue;
                            }
                            for (i, s) in t.iter_mut().enumerate() {
                                *s += a * (std::f32::consts::TAU * h as f32 * i as f32 / TABLE as f32).sin();
                            }
                        }
                        let peak = t.iter().fold(0.0f32, |m, x| m.max(x.abs())).max(1e-6);
                        t.iter_mut().for_each(|x| *x /= peak);
                        t
                    })
                    .collect()
            })
            .collect()
    })
}

struct Voice {
    env: Adsr,
    filter: [Svf; 2],
    phases: [f32; MAX_UNISON],
    freq: f32,
    gain: f32,
    id: NoteId,
    gate: bool,
    active: bool,
    age: u64,
}

pub struct Tide {
    sr: f32,
    p: ParamStore<12>,
    voices: Vec<Voice>,
    tables: &'static Vec<Vec<Vec<f32>>>,
    lfo: f32,
    age: u64,
}

impl Tide {
    pub fn new(sample_rate: f32) -> Self {
        let voices = (0..VOICES)
            .map(|i| Voice {
                env: Adsr::new(sample_rate),
                filter: [Svf::default(), Svf::default()],
                // Unison oscillators start at spread phases so they don't phase-cancel.
                phases: std::array::from_fn(|u| ((i * 7 + u * 3) % 11) as f32 / 11.0),
                freq: 440.0,
                gain: 0.0,
                id: 0,
                gate: false,
                active: false,
                age: 0,
            })
            .collect();
        Self { sr: sample_rate, p: ParamStore::new(&PARAMS), voices, tables: tables(), lfo: 0.0, age: 1 }
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let n = range.len();
        let lfo_inc = self.p.get(param::LFO_RATE) / self.sr;
        let pos0 = self.p.get(param::POSITION);
        let lfo_amt = self.p.get(param::LFO_AMOUNT);
        let unison = (self.p.get(param::UNISON) as usize).clamp(1, MAX_UNISON);
        let detune = self.p.get(param::DETUNE);
        let (cut, res, vol) = (self.p.get(param::CUTOFF), self.p.get(param::RESONANCE), self.p.get(param::VOLUME));
        let norm = 1.0 / (unison as f32).sqrt();
        // The LFO advances once per segment block (smooth enough for a slow morph).
        let lfo = (self.lfo * std::f32::consts::TAU).sin();
        self.lfo = (self.lfo + lfo_inc * n as f32).fract();
        let pos = (pos0 + lfo * lfo_amt * 0.5).clamp(0.0, 1.0) * (FRAMES - 1) as f32;
        let (f0, mix) = (pos.floor() as usize, pos.fract());
        let f1 = (f0 + 1).min(FRAMES - 1);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            // Pick the mip level whose highest harmonic stays below Nyquist.
            let top = (v.freq * (1.0 + detune / 1200.0)).max(1.0);
            let mut level = 0;
            while level + 1 < LEVELS && top * (512usize >> level) as f32 > self.sr * 0.45 {
                level += 1;
            }
            let (ta, tb) = (&self.tables[level][f0], &self.tables[level][f1]);
            v.filter[0].set(cut, res, self.sr);
            v.filter[1].set(cut, res, self.sr);
            let incs: [f32; MAX_UNISON] = std::array::from_fn(|u| {
                let spread = if unison == 1 { 0.0 } else { u as f32 / (unison - 1) as f32 * 2.0 - 1.0 };
                v.freq * 2f32.powf(spread * detune / 1200.0) / self.sr
            });
            for i in range.clone() {
                let (mut sl, mut sr) = (0.0f32, 0.0f32);
                for (u, (phase, inc)) in v.phases.iter_mut().zip(incs).take(unison).enumerate() {
                    let x = *phase * TABLE as f32;
                    let i0 = x as usize % TABLE;
                    let fr = x.fract();
                    let i1 = (i0 + 1) % TABLE;
                    let a = ta[i0] + (ta[i1] - ta[i0]) * fr;
                    let b = tb[i0] + (tb[i1] - tb[i0]) * fr;
                    let s = a + (b - a) * mix;
                    // Spread unison voices across the stereo field.
                    let pan = if unison == 1 { 0.5 } else { u as f32 / (unison - 1) as f32 };
                    sl += s * (1.0 - pan * 0.6);
                    sr += s * (0.4 + pan * 0.6);
                    *phase = (*phase + inc).fract();
                }
                let e = v.env.next() * v.gain * vol * norm;
                l[i] += v.filter[0].process(sl, SvfMode::LowPass) * e;
                r[i] += v.filter[1].process(sr, SvfMode::LowPass) * e;
                if v.env.is_idle() {
                    v.active = false;
                    break;
                }
            }
        }
    }

    fn handle(&mut self, ev: NoteEvent) {
        match ev {
            NoteEvent::On { id, key, velocity } => {
                let i = pick_voice(&self.voices, |v| !v.active, |v| v.age);
                self.age += 1;
                let adsr = AdsrParams {
                    attack: self.p.get(param::ATTACK),
                    decay: self.p.get(param::DECAY),
                    sustain: self.p.get(param::SUSTAIN),
                    release: self.p.get(param::RELEASE),
                };
                let v = &mut self.voices[i];
                v.env.set_params(adsr);
                v.env.gate_on();
                v.freq = soundesti_dsp::midi_to_freq(key as f32);
                v.gain = velocity.clamp(0.0, 1.0);
                v.id = id;
                v.gate = true;
                v.active = true;
                v.age = self.age;
                v.filter.iter_mut().for_each(Svf::reset);
            }
            NoteEvent::Off { id } => {
                for v in self.voices.iter_mut().filter(|v| v.gate && v.id == id) {
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
                    v.active = false;
                }
            }
        }
    }
}

impl Instrument for Tide {
    fn type_id(&self) -> &'static str {
        TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Tide"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &PARAMS
    }
    fn get_param(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set_param(&mut self, id: u32, value: f32) {
        self.p.set(id, value);
    }
    fn process(&mut self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut ctx = (self, left, right);
        for_each_segment(&mut ctx, events, n, |(s, _, _), ev| s.handle(ev), |(s, l, r), range| s.render(range, l, r));
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(t: &mut Tide, key: u8, frames: usize) -> Vec<f32> {
        let (mut l, mut r) = (vec![0.0; frames], vec![0.0; frames]);
        t.process(&[TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key, velocity: 1.0 } }], &mut l, &mut r);
        l
    }

    #[test]
    fn plays_in_tune_and_position_changes_the_timbre() {
        let sr = 48_000.0;
        let mut t = Tide::new(sr);
        t.set_param(param::POSITION, 0.0); // pure sine
        let out = play(&mut t, 69, 48_000);
        let crossings = out[4_800..].windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        assert!((crossings as i32 - 396).abs() <= 3, "A4 = 440 Hz over 0.9 s: {crossings}");
        // Brighter frames have more high-frequency content (sum of sample-to-sample changes).
        let rough = |v: &[f32]| {
            v.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>() / v.iter().map(|x| x.abs()).sum::<f32>().max(1e-9)
        };
        let mut t2 = Tide::new(sr);
        t2.set_param(param::POSITION, 1.0);
        assert!(rough(&play(&mut t2, 57, 9_600)[2_000..]) > rough(&play(&mut Tide::new(sr), 57, 9_600)[2_000..]) * 1.5);
    }

    #[test]
    fn high_notes_stay_band_limited_and_bounded() {
        let mut t = Tide::new(48_000.0);
        t.set_param(param::POSITION, 0.86); // saw-to-pulse region
        t.set_param(param::UNISON, 5.0);
        let out = play(&mut t, 120, 9_600);
        assert!(out.iter().all(|x| x.is_finite() && x.abs() < 2.0));
    }
}
