//! Sampler — plays an audio file (the channel sampler role): root key, pitch tracking,
//! one-shot or ADSR-gated playback, reverse, start offset. Sample data arrives through
//! `Instrument::set_sample` (decoded off the audio thread).

use std::sync::Arc;

use soundesti_dsp::{Adsr, AdsrParams};
use soundesti_plugin_api::{Instrument, NoteEvent, NoteId, ParamInfo, SampleData, TimedEvent, for_each_segment};

use crate::common::{ParamStore, cont, pick_voice, semis, stepped};

pub const TYPE_ID: &str = "soundesti.sampler";
const VOICES: usize = 16;

pub mod param {
    pub const ROOT: u32 = 0;
    pub const TRACK: u32 = 1;
    pub const ONE_SHOT: u32 = 2;
    pub const REVERSE: u32 = 3;
    pub const START: u32 = 4;
    pub const ATTACK: u32 = 5;
    pub const DECAY: u32 = 6;
    pub const SUSTAIN: u32 = 7;
    pub const RELEASE: u32 = 8;
    pub const TUNE: u32 = 9;
    pub const VOLUME: u32 = 10;
}

pub static PARAMS: [ParamInfo; 11] = [
    stepped(param::ROOT, "sample.root", "Root key", "Sample", 0.0, 127.0, 60.0, &[]),
    stepped(param::TRACK, "sample.track", "Key tracking", "Sample", 0.0, 1.0, 1.0, &["Off", "On"]),
    stepped(param::ONE_SHOT, "sample.oneshot", "Playback", "Sample", 0.0, 1.0, 1.0, &["Gated", "One-shot"]),
    stepped(param::REVERSE, "sample.reverse", "Reverse", "Sample", 0.0, 1.0, 0.0, &["Off", "On"]),
    cont(param::START, "sample.start", "Start", "Sample", 0.0, 0.99, 0.0, ""),
    cont(param::ATTACK, "amp.attack", "Attack", "Envelope", 0.0, 5.0, 0.0, "s"),
    cont(param::DECAY, "amp.decay", "Decay", "Envelope", 0.0, 5.0, 0.5, "s"),
    cont(param::SUSTAIN, "amp.sustain", "Sustain", "Envelope", 0.0, 1.0, 1.0, ""),
    cont(param::RELEASE, "amp.release", "Release", "Envelope", 0.0, 5.0, 0.05, "s"),
    cont(param::TUNE, "sample.tune", "Tune", "Sample", -24.0, 24.0, 0.0, "st"),
    cont(param::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.8, ""),
];

struct Voice {
    env: Adsr,
    pos: f64,
    step: f64,
    reverse: bool,
    gain: f32,
    id: NoteId,
    gate: bool,
    active: bool,
    age: u64,
}

pub struct Sampler {
    sr: f32,
    p: ParamStore<11>,
    sample: Option<Arc<SampleData>>,
    voices: Vec<Voice>,
    age: u64,
}

impl Sampler {
    pub fn new(sample_rate: f32) -> Self {
        let voices = (0..VOICES)
            .map(|_| Voice {
                env: Adsr::new(sample_rate),
                pos: 0.0,
                step: 1.0,
                reverse: false,
                gain: 0.0,
                id: 0,
                gate: false,
                active: false,
                age: 0,
            })
            .collect();
        Self { sr: sample_rate, p: ParamStore::new(&PARAMS), sample: None, voices, age: 1 }
    }

    fn adsr(&self) -> AdsrParams {
        AdsrParams {
            attack: self.p.get(param::ATTACK),
            decay: self.p.get(param::DECAY),
            sustain: self.p.get(param::SUSTAIN),
            release: self.p.get(param::RELEASE),
        }
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let Some(s) = self.sample.as_ref() else { return };
        let len = s.len();
        if len < 2 {
            return;
        }
        let vol = self.p.get(param::VOLUME);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in range.clone() {
                let idx = v.pos as usize;
                if idx + 1 >= len || v.pos < 0.0 {
                    v.active = false;
                    break;
                }
                let frac = (v.pos - idx as f64) as f32;
                let sl = s.left[idx] + (s.left[idx + 1] - s.left[idx]) * frac;
                let sr = s.right[idx] + (s.right[idx + 1] - s.right[idx]) * frac;
                let e = v.env.next();
                l[i] += sl * e * v.gain * vol;
                r[i] += sr * e * v.gain * vol;
                v.pos += if v.reverse { -v.step } else { v.step };
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
                let Some(s) = self.sample.as_ref() else { return };
                let len = s.len();
                if len < 2 {
                    return;
                }
                let track = self.p.get(param::TRACK) > 0.5;
                let semitones =
                    if track { key as f32 - self.p.get(param::ROOT) } else { 0.0 } + self.p.get(param::TUNE);
                let step = (s.sample_rate / self.sr) as f64 * semis(semitones) as f64;
                let reverse = self.p.get(param::REVERSE) > 0.5;
                let start = (self.p.get(param::START) as f64 * (len - 1) as f64).floor();
                let i = pick_voice(&self.voices, |v| !v.active, |v| v.age);
                self.age += 1;
                let adsr = self.adsr();
                let v = &mut self.voices[i];
                v.env.set_params(adsr);
                v.env.gate_on();
                v.pos = if reverse { (len - 2) as f64 - start } else { start };
                v.step = step;
                v.reverse = reverse;
                v.gain = velocity.clamp(0.0, 1.0);
                v.id = id;
                v.gate = true;
                v.active = true;
                v.age = self.age;
            }
            NoteEvent::Off { id } => {
                // One-shot samples ignore note-off (drum hits ring to the end).
                if self.p.get(param::ONE_SHOT) > 0.5 {
                    return;
                }
                for v in self.voices.iter_mut().filter(|v| v.gate && v.id == id) {
                    v.gate = false;
                    v.env.gate_off();
                }
            }
            NoteEvent::AllNotesOff => {
                for v in self.voices.iter_mut() {
                    v.gate = false;
                    v.env.gate_off();
                }
            }
            NoteEvent::AllSoundOff => {
                for v in self.voices.iter_mut() {
                    v.gate = false;
                    v.env.kill();
                }
            }
        }
    }
}

impl Instrument for Sampler {
    fn type_id(&self) -> &'static str {
        TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Sampler"
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
    fn set_sample(&mut self, sample: Option<Arc<SampleData>>) -> Option<Arc<SampleData>> {
        // Voices still reading the old sample must stop before it is released.
        for v in &mut self.voices {
            v.active = false;
        }
        std::mem::replace(&mut self.sample, sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: f32, secs: f32) -> Arc<SampleData> {
        let n = (sr * secs) as usize;
        let v: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * freq * i as f32 / sr).sin() * 0.5).collect();
        Arc::new(SampleData { left: v.clone(), right: v, sample_rate: sr, name: "sine".into() })
    }

    fn play(s: &mut Sampler, key: u8, frames: usize) -> Vec<f32> {
        let mut l = vec![0.0; frames];
        let mut r = vec![0.0; frames];
        s.process(&[TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key, velocity: 1.0 } }], &mut l, &mut r);
        l
    }

    fn crossings(v: &[f32]) -> i64 {
        v.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() as i64
    }

    #[test]
    fn silent_without_sample_and_plays_with_one() {
        let mut s = Sampler::new(48_000.0);
        assert!(play(&mut s, 60, 512).iter().all(|&x| x == 0.0));
        assert!(s.set_sample(Some(sine(440.0, 48_000.0, 1.0))).is_none());
        let out = play(&mut s, 60, 48_000);
        assert!((crossings(&out) - 440).abs() <= 2, "root key plays at original pitch");
    }

    #[test]
    fn key_tracking_and_resampling() {
        let mut s = Sampler::new(48_000.0);
        // 44.1 kHz sample on a 48 kHz engine must keep its pitch.
        s.set_sample(Some(sine(441.0, 44_100.0, 2.0)));
        let out = play(&mut s, 72, 24_000); // one octave up, 0.5 s
        assert!((crossings(&out) - 441).abs() <= 3, "octave up doubles the pitch: {}", crossings(&out));
    }

    #[test]
    fn one_shot_plays_to_end_then_stops() {
        let mut s = Sampler::new(48_000.0);
        s.set_sample(Some(sine(440.0, 48_000.0, 0.1)));
        play(&mut s, 60, 256);
        let mut l = vec![0.0; 10_000];
        let mut r = vec![0.0; 10_000];
        s.process(&[TimedEvent { offset: 0, event: NoteEvent::Off { id: 1 } }], &mut l, &mut r);
        assert!(l[..2_000].iter().any(|x| x.abs() > 0.1), "note-off must not cut a one-shot");
        assert_eq!(s.active_voices(), 0, "voice ends at the sample's end");
    }

    #[test]
    fn replacing_the_sample_returns_the_old_one() {
        let mut s = Sampler::new(48_000.0);
        let a = sine(100.0, 48_000.0, 0.1);
        s.set_sample(Some(a.clone()));
        let old = s.set_sample(None).unwrap();
        assert!(Arc::ptr_eq(&old, &a));
    }
}
