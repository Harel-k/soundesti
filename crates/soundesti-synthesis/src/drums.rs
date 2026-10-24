//! Drum synthesizers for the default kit: Kickr (kick), Clapper (snare/clap) and
//! Hatz (hi-hats). One-shot instruments: note-off is ignored, hits ring out.
//! Original designs; they fill the roles of FL's default sample kit without samples.

use soundesti_dsp::{Noise, Svf, SvfMode};
use soundesti_plugin_api::{Instrument, NoteEvent, ParamInfo, TimedEvent, for_each_segment};

use crate::common::{ParamStore, cont, decay_coef, pick_voice, semis, stepped};

const VOICES: usize = 8;

// ---------------------------------------------------------------------------
// Kickr

pub mod kick {
    pub const TYPE_ID: &str = "soundesti.kickr";
    pub const PITCH_START: u32 = 0;
    pub const PITCH_END: u32 = 1;
    pub const PITCH_DECAY: u32 = 2;
    pub const AMP_DECAY: u32 = 3;
    pub const CLICK: u32 = 4;
    pub const DRIVE: u32 = 5;
    pub const VOLUME: u32 = 6;
}

pub static KICK_PARAMS: [ParamInfo; 7] = [
    cont(kick::PITCH_START, "pitch.start", "Start pitch", "Pitch", 40.0, 1000.0, 190.0, "Hz"),
    cont(kick::PITCH_END, "pitch.end", "End pitch", "Pitch", 20.0, 200.0, 48.0, "Hz"),
    cont(kick::PITCH_DECAY, "pitch.decay", "Pitch decay", "Pitch", 0.005, 0.5, 0.055, "s"),
    cont(kick::AMP_DECAY, "amp.decay", "Decay", "Amp", 0.05, 2.0, 0.45, "s"),
    cont(kick::CLICK, "amp.click", "Click", "Amp", 0.0, 1.0, 0.35, ""),
    cont(kick::DRIVE, "amp.drive", "Drive", "Amp", 0.0, 1.0, 0.3, ""),
    cont(kick::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.85, ""),
];

#[derive(Clone, Copy, Default)]
struct KickVoice {
    active: bool,
    phase: f32,
    t: u32,
    amp: f32,
    pitch_env: f32,
    gain: f32,
    pitch_ratio: f32,
    age: u64,
}

pub struct Kickr {
    sr: f32,
    p: ParamStore<7>,
    voices: [KickVoice; VOICES],
    noise: Noise,
    age: u64,
}

impl Kickr {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sr: sample_rate,
            p: ParamStore::new(&KICK_PARAMS),
            voices: [KickVoice::default(); VOICES],
            noise: Noise::new(77),
            age: 1,
        }
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let (ps, pe, pd, ad) = (
            self.p.get(kick::PITCH_START),
            self.p.get(kick::PITCH_END),
            self.p.get(kick::PITCH_DECAY),
            self.p.get(kick::AMP_DECAY),
        );
        let (click, drive, vol) = (self.p.get(kick::CLICK), self.p.get(kick::DRIVE), self.p.get(kick::VOLUME));
        let pitch_coef = decay_coef(pd, self.sr);
        let amp_coef = decay_coef(ad, self.sr);
        let click_len = (0.004 * self.sr) as u32;
        let drive_k = 1.0 + drive * 8.0;
        let norm = drive_k.tanh();
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let mut pitch_env = v.pitch_env;
            for i in range.clone() {
                let f = (pe + (ps - pe) * pitch_env) * v.pitch_ratio;
                v.phase = (v.phase + f / self.sr).fract();
                let mut y = (v.phase * std::f32::consts::TAU).sin() * v.amp;
                if v.t < click_len {
                    y += self.noise.next() * click * (1.0 - v.t as f32 / click_len as f32);
                }
                y = (y * drive_k).tanh() / norm * v.gain * vol;
                l[i] += y;
                r[i] += y;
                v.amp *= amp_coef;
                pitch_env *= pitch_coef;
                v.t += 1;
                if v.amp < 1e-4 {
                    v.active = false;
                    break;
                }
            }
            v.pitch_env = pitch_env;
        }
    }
}

impl Instrument for Kickr {
    fn type_id(&self) -> &'static str {
        kick::TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Kickr"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &KICK_PARAMS
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
        for_each_segment(
            &mut ctx,
            events,
            n,
            |(s, _, _), ev| match ev {
                NoteEvent::On { key, velocity, .. } => {
                    let i = pick_voice(&s.voices, |v| !v.active, |v| v.age);
                    s.age += 1;
                    s.voices[i] = KickVoice {
                        active: true,
                        phase: 0.0,
                        t: 0,
                        amp: 1.0,
                        pitch_env: 1.0,
                        gain: velocity.clamp(0.0, 1.0),
                        pitch_ratio: semis(key as f32 - 60.0),
                        age: s.age,
                    };
                }
                NoteEvent::AllSoundOff => s.voices.iter_mut().for_each(|v| v.active = false),
                _ => {}
            },
            |(s, l, r), range| s.render(range, l, r),
        );
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

// ---------------------------------------------------------------------------
// Clapper (snare / clap)

pub mod clap {
    pub const TYPE_ID: &str = "soundesti.clapper";
    pub const TONE: u32 = 0;
    pub const TONE_MIX: u32 = 1;
    pub const NOISE_TONE: u32 = 2;
    pub const DECAY: u32 = 3;
    pub const SNAPPY: u32 = 4;
    pub const CLAPS: u32 = 5;
    pub const VOLUME: u32 = 6;
}

const CLAP_LABELS: &[&str] = &["Snare", "Clap 1", "Clap 2", "Clap 3", "Clap 4"];

pub static CLAP_PARAMS: [ParamInfo; 7] = [
    cont(clap::TONE, "body.tone", "Body tone", "Body", 100.0, 400.0, 185.0, "Hz"),
    cont(clap::TONE_MIX, "body.mix", "Body", "Body", 0.0, 1.0, 0.45, ""),
    cont(clap::NOISE_TONE, "noise.tone", "Noise tone", "Noise", 800.0, 12_000.0, 4_200.0, "Hz"),
    cont(clap::DECAY, "amp.decay", "Decay", "Amp", 0.03, 1.0, 0.17, "s"),
    cont(clap::SNAPPY, "noise.snappy", "Snappy", "Noise", 0.0, 1.0, 0.8, ""),
    stepped(clap::CLAPS, "mode.claps", "Mode", "Mode", 0.0, 4.0, 0.0, CLAP_LABELS),
    cont(clap::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.8, ""),
];

#[derive(Clone, Default)]
struct ClapVoice {
    active: bool,
    t: u32,
    phase: f32,
    amp: f32,
    body_env: f32,
    drop_env: f32,
    burst_env: f32,
    gain: f32,
    pitch_ratio: f32,
    filter: Svf,
    age: u64,
}

pub struct Clapper {
    sr: f32,
    p: ParamStore<7>,
    voices: [ClapVoice; VOICES],
    noise: Noise,
    age: u64,
}

impl Clapper {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sr: sample_rate,
            p: ParamStore::new(&CLAP_PARAMS),
            voices: std::array::from_fn(|_| ClapVoice::default()),
            noise: Noise::new(4242),
            age: 1,
        }
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let (tone, mix, ntone, decay) =
            (self.p.get(clap::TONE), self.p.get(clap::TONE_MIX), self.p.get(clap::NOISE_TONE), self.p.get(clap::DECAY));
        let (snappy, claps, vol) = (self.p.get(clap::SNAPPY), self.p.get(clap::CLAPS) as u32, self.p.get(clap::VOLUME));
        let amp_coef = decay_coef(decay, self.sr);
        let burst_gap = (0.011 * self.sr) as u32;
        let burst_coef = decay_coef(0.009, self.sr);
        let body_coef = decay_coef(decay * 0.6, self.sr);
        let drop_coef = decay_coef(decay * 0.08, self.sr);
        let gap = burst_gap.max(1);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.filter.set(ntone * v.pitch_ratio, 1.2, self.sr);
            for i in range.clone() {
                // Body: short sine with a small pitch drop.
                let f = tone * v.pitch_ratio * (1.0 + 0.5 * v.drop_env);
                v.phase = (v.phase + f / self.sr).fract();
                let body = (v.phase * std::f32::consts::TAU).sin() * v.body_env;
                v.body_env *= body_coef;
                v.drop_env *= drop_coef;
                let n = v.filter.process(self.noise.next(), SvfMode::BandPass);
                let noise_env = if claps > 0 {
                    // Clap: a few quick bursts (each retriggers), then the tail.
                    if v.t % gap == 0 && v.t / gap < claps {
                        v.burst_env = 1.0;
                    }
                    let e = if v.t / gap < claps { v.burst_env } else { v.amp };
                    v.burst_env *= burst_coef;
                    e
                } else {
                    v.amp
                };
                let body_mix = if claps > 0 { mix * 0.15 } else { mix };
                let y = (body * body_mix + n * snappy * noise_env * 2.5) * v.gain * vol;
                l[i] += y;
                r[i] += y;
                v.amp *= amp_coef;
                v.t += 1;
                if v.amp < 1e-4 && v.t > burst_gap * claps {
                    v.active = false;
                    break;
                }
            }
        }
    }
}

impl Instrument for Clapper {
    fn type_id(&self) -> &'static str {
        clap::TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Clapper"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &CLAP_PARAMS
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
        for_each_segment(
            &mut ctx,
            events,
            n,
            |(s, _, _), ev| match ev {
                NoteEvent::On { key, velocity, .. } => {
                    let i = pick_voice(&s.voices, |v| !v.active, |v| v.age);
                    s.age += 1;
                    let v = &mut s.voices[i];
                    v.active = true;
                    v.t = 0;
                    v.phase = 0.0;
                    v.amp = 1.0;
                    v.body_env = 1.0;
                    v.drop_env = 1.0;
                    v.burst_env = 0.0;
                    v.gain = velocity.clamp(0.0, 1.0);
                    v.pitch_ratio = semis((key as f32 - 60.0) * 0.5);
                    v.filter.reset();
                    v.age = s.age;
                }
                NoteEvent::AllSoundOff => s.voices.iter_mut().for_each(|v| v.active = false),
                _ => {}
            },
            |(s, l, r), range| s.render(range, l, r),
        );
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

// ---------------------------------------------------------------------------
// Hatz (hi-hats)

pub mod hat {
    pub const TYPE_ID: &str = "soundesti.hatz";
    pub const TONE: u32 = 0;
    pub const DECAY: u32 = 1;
    pub const METAL: u32 = 2;
    pub const CHOKE: u32 = 3;
    pub const VOLUME: u32 = 4;
}

pub static HAT_PARAMS: [ParamInfo; 5] = [
    cont(hat::TONE, "tone.cutoff", "Tone", "Tone", 3_000.0, 16_000.0, 7_500.0, "Hz"),
    cont(hat::DECAY, "amp.decay", "Decay", "Amp", 0.02, 1.5, 0.07, "s"),
    cont(hat::METAL, "tone.metal", "Metal", "Tone", 0.0, 1.0, 0.65, ""),
    stepped(hat::CHOKE, "mode.choke", "Choke", "Mode", 0.0, 1.0, 1.0, &["Off", "On"]),
    cont(hat::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.6, ""),
];

/// Inharmonic square-wave bank ratios (classic metallic cymbal recipe).
const HAT_FREQS: [f32; 6] = [205.3, 304.4, 369.6, 522.7, 540.0, 800.0];

#[derive(Clone, Default)]
struct HatVoice {
    active: bool,
    phases: [f32; 6],
    amp: f32,
    gain: f32,
    ratio: f32,
    choke: f32,
    hp: Svf,
    age: u64,
}

pub struct Hatz {
    sr: f32,
    p: ParamStore<5>,
    voices: [HatVoice; VOICES],
    noise: Noise,
    age: u64,
}

impl Hatz {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sr: sample_rate,
            p: ParamStore::new(&HAT_PARAMS),
            voices: std::array::from_fn(|_| HatVoice::default()),
            noise: Noise::new(9001),
            age: 1,
        }
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let (tone, decay, metal, vol) =
            (self.p.get(hat::TONE), self.p.get(hat::DECAY), self.p.get(hat::METAL), self.p.get(hat::VOLUME));
        let amp_coef = decay_coef(decay, self.sr);
        let choke_coef = decay_coef(0.004, self.sr);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            v.hp.set(tone, 0.8, self.sr);
            for i in range.clone() {
                let mut sq = 0.0;
                for (ph, f) in v.phases.iter_mut().zip(HAT_FREQS) {
                    *ph = (*ph + f * 4.0 * v.ratio / self.sr).fract();
                    sq += if *ph < 0.5 { 1.0 } else { -1.0 };
                }
                let src = sq / 6.0 * metal + self.noise.next() * (1.0 - metal);
                let y = v.hp.process(src, SvfMode::HighPass) * v.amp * v.choke * v.gain * vol;
                l[i] += y;
                r[i] += y;
                v.amp *= amp_coef;
                if v.choke < 1.0 {
                    v.choke *= choke_coef;
                }
                if v.amp * v.choke < 1e-4 {
                    v.active = false;
                    break;
                }
            }
        }
    }
}

impl Instrument for Hatz {
    fn type_id(&self) -> &'static str {
        hat::TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Hatz"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &HAT_PARAMS
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
        for_each_segment(
            &mut ctx,
            events,
            n,
            |(s, _, _), ev| match ev {
                NoteEvent::On { key, velocity, .. } => {
                    if s.p.get(hat::CHOKE) > 0.5 {
                        // A new hit chokes ringing ones (closed hat cuts the open hat).
                        for v in s.voices.iter_mut().filter(|v| v.active) {
                            v.choke = v.choke.min(0.999);
                        }
                    }
                    let i = pick_voice(&s.voices, |v| !v.active, |v| v.age);
                    s.age += 1;
                    let v = &mut s.voices[i];
                    v.active = true;
                    v.amp = 1.0;
                    v.choke = 1.0;
                    v.gain = velocity.clamp(0.0, 1.0);
                    v.ratio = semis((key as f32 - 60.0) * 0.25);
                    v.hp.reset();
                    v.age = s.age;
                }
                NoteEvent::AllSoundOff => s.voices.iter_mut().for_each(|v| v.active = false),
                _ => {}
            },
            |(s, l, r), range| s.render(range, l, r),
        );
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| v.active).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_plugin_api::NoteId;

    const SR: f32 = 48_000.0;

    fn hit(inst: &mut dyn Instrument, frames: usize) -> Vec<f32> {
        let mut l = vec![0.0; frames];
        let mut r = vec![0.0; frames];
        let on = [TimedEvent { offset: 0, event: NoteEvent::On { id: 1 as NoteId, key: 60, velocity: 1.0 } }];
        inst.process(&on, &mut l, &mut r);
        l
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0f32, |a, x| a.max(x.abs()))
    }

    fn check_drum(mut inst: Box<dyn Instrument>, name: &str) {
        let a = hit(inst.as_mut(), 4800);
        assert!(peak(&a) > 0.05, "{name} too quiet: {}", peak(&a));
        assert!(peak(&a) < 1.5, "{name} too loud: {}", peak(&a));
        assert!(a.iter().all(|x| x.is_finite()), "{name} produced NaN");
        // One-shot: rings out on its own and goes silent.
        let mut l = vec![0.0; 96_000];
        let mut r = vec![0.0; 96_000];
        inst.process(&[], &mut l, &mut r);
        assert_eq!(inst.active_voices(), 0, "{name} never finished");
        // Panic stops everything.
        hit(inst.as_mut(), 64);
        let mut l = vec![0.0; 64];
        let mut r = vec![0.0; 64];
        inst.process(&[TimedEvent { offset: 0, event: NoteEvent::AllSoundOff }], &mut l, &mut r);
        assert_eq!(inst.active_voices(), 0);
    }

    #[test]
    fn kick_snare_clap_hat_sound_and_stop() {
        check_drum(Box::new(Kickr::new(SR)), "kick");
        check_drum(Box::new(Clapper::new(SR)), "snare");
        let mut clap = Clapper::new(SR);
        clap.set_param(clap::CLAPS, 3.0);
        check_drum(Box::new(clap), "clap");
        check_drum(Box::new(Hatz::new(SR)), "hat");
    }

    #[test]
    fn kick_is_low_and_hat_is_bright() {
        // Count zero crossings in the body of each sound as a rough brightness measure.
        let zc = |v: &[f32]| v.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        let kick = hit(&mut Kickr::new(SR), 9_600);
        let hat = hit(&mut Hatz::new(SR), 2_400);
        assert!(zc(&kick[2_400..9_600]) < 40, "kick should be low-pitched");
        assert!(zc(&hat) > 200, "hat should be bright");
    }

    #[test]
    fn choke_cuts_previous_hat() {
        let mut h = Hatz::new(SR);
        h.set_param(hat::DECAY, 1.2); // open hat
        hit(&mut h, 256);
        hit(&mut h, 2_400); // second hit chokes the first within a few ms
        assert_eq!(h.active_voices(), 1);
    }
}
