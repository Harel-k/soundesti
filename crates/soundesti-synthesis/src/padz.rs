//! Padz — 16-pad drum sampler (the role of a pad-based drum machine). Each pad has its own
//! sample, volume, pan and tune. Pads answer two key ranges: C5.. (pad 1 = C5, the step
//! sequencer's default note) and the General MIDI drum range starting at C3 (key 36).
//! Original design and code.

use std::sync::Arc;

use soundesti_plugin_api::{Instrument, NoteEvent, ParamInfo, SampleData, TimedEvent, for_each_segment};

use crate::common::{ParamStore, cont, pick_voice, semis};

pub const TYPE_ID: &str = "soundesti.padz";
pub const PADS: usize = 16;
const VOICES: usize = 32;
/// Key of pad 1 in the main range, and in the General MIDI drum range.
pub const FIRST_KEY: u8 = 60;
pub const GM_FIRST_KEY: u8 = 36;

/// Parameter ids: pad `i` has volume `i*3`, pan `i*3+1`, tune `i*3+2`; master volume 48.
pub const MASTER: u32 = 48;

pub fn volume_id(pad: usize) -> u32 {
    pad as u32 * 3
}
pub fn pan_id(pad: usize) -> u32 {
    pad as u32 * 3 + 1
}
pub fn tune_id(pad: usize) -> u32 {
    pad as u32 * 3 + 2
}

macro_rules! pad_params {
    ($($n:literal),*) => {
        [
            $(
                cont(($n - 1) * 3, concat!("pad", $n, ".volume"), concat!("Pad ", $n, " volume"), concat!("Pad ", $n), 0.0, 1.0, 0.8, ""),
                cont(($n - 1) * 3 + 1, concat!("pad", $n, ".pan"), concat!("Pad ", $n, " pan"), concat!("Pad ", $n), -1.0, 1.0, 0.0, ""),
                cont(($n - 1) * 3 + 2, concat!("pad", $n, ".tune"), concat!("Pad ", $n, " tune"), concat!("Pad ", $n), -24.0, 24.0, 0.0, "st"),
            )*
            cont(MASTER, "master.volume", "Volume", "Master", 0.0, 1.0, 0.8, ""),
        ]
    };
}

pub static PARAMS: [ParamInfo; 49] = pad_params!(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16);

/// Which pad a key plays, if any.
pub fn pad_for_key(key: u8) -> Option<usize> {
    let k = key as usize;
    let (main, gm) = (FIRST_KEY as usize, GM_FIRST_KEY as usize);
    if (main..main + PADS).contains(&k) {
        Some(k - main)
    } else if (gm..gm + PADS).contains(&k) {
        Some(k - gm)
    } else {
        None
    }
}

struct Voice {
    pad: usize,
    pos: f64,
    step: f64,
    gain: [f32; 2],
    active: bool,
    age: u64,
}

pub struct Padz {
    sr: f32,
    p: ParamStore<49>,
    samples: [Option<Arc<SampleData>>; PADS],
    voices: Vec<Voice>,
    age: u64,
}

impl Padz {
    pub fn new(sample_rate: f32) -> Self {
        let voices =
            (0..VOICES).map(|_| Voice { pad: 0, pos: 0.0, step: 1.0, gain: [0.0; 2], active: false, age: 0 }).collect();
        Self { sr: sample_rate, p: ParamStore::new(&PARAMS), samples: Default::default(), voices, age: 1 }
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let master = self.p.get(MASTER);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let Some(s) = self.samples[v.pad].as_ref() else {
                v.active = false;
                continue;
            };
            let len = s.len();
            for i in range.clone() {
                let idx = v.pos as usize;
                if idx + 1 >= len {
                    v.active = false;
                    break;
                }
                let frac = (v.pos - idx as f64) as f32;
                let sl = s.left[idx] + (s.left[idx + 1] - s.left[idx]) * frac;
                let sr = s.right[idx] + (s.right[idx + 1] - s.right[idx]) * frac;
                l[i] += sl * v.gain[0] * master;
                r[i] += sr * v.gain[1] * master;
                v.pos += v.step;
            }
        }
    }

    fn handle(&mut self, ev: NoteEvent) {
        match ev {
            NoteEvent::On { key, velocity, .. } => {
                let Some(pad) = pad_for_key(key) else { return };
                let Some(s) = self.samples[pad].as_ref() else { return };
                let vol = self.p.get(volume_id(pad)) * velocity.clamp(0.0, 1.0);
                let (gl, gr) = soundesti_dsp::balance_pan(self.p.get(pan_id(pad)));
                let step = (s.sample_rate / self.sr) as f64 * semis(self.p.get(tune_id(pad))) as f64;
                let i = pick_voice(&self.voices, |v| !v.active, |v| v.age);
                self.age += 1;
                self.voices[i] = Voice { pad, pos: 0.0, step, gain: [vol * gl, vol * gr], active: true, age: self.age };
            }
            // Pads are one-shots: they ring to the end of their sample.
            NoteEvent::Off { .. } | NoteEvent::AllNotesOff => {}
            NoteEvent::AllSoundOff => self.voices.iter_mut().for_each(|v| v.active = false),
        }
    }
}

impl Instrument for Padz {
    fn type_id(&self) -> &'static str {
        TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Padz"
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
        self.set_sample_at(0, sample)
    }
    fn set_sample_at(&mut self, index: usize, sample: Option<Arc<SampleData>>) -> Option<Arc<SampleData>> {
        if index >= PADS {
            return sample;
        }
        // Voices reading the old sample stop before it is released.
        for v in self.voices.iter_mut().filter(|v| v.pad == index) {
            v.active = false;
        }
        std::mem::replace(&mut self.samples[index], sample)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn click(value: f32, n: usize) -> Arc<SampleData> {
        Arc::new(SampleData { left: vec![value; n], right: vec![value; n], sample_rate: 48_000.0, name: "c".into() })
    }

    fn hit(p: &mut Padz, key: u8) -> f32 {
        let (mut l, mut r) = (vec![0.0; 256], vec![0.0; 256]);
        p.process(&[TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key, velocity: 1.0 } }], &mut l, &mut r);
        p.process(&[TimedEvent { offset: 0, event: NoteEvent::AllSoundOff }], &mut [0.0; 1], &mut [0.0; 1]);
        l[10]
    }

    #[test]
    fn each_pad_plays_its_own_sample_from_both_key_ranges() {
        let mut p = Padz::new(48_000.0);
        assert!(p.set_sample_at(0, Some(click(0.5, 1000))).is_none());
        p.set_sample_at(5, Some(click(0.25, 1000)));
        p.set_param(MASTER, 1.0);
        p.set_param(pan_id(0), 0.0);
        let a = hit(&mut p, 60);
        let gm = hit(&mut p, 36);
        let f = hit(&mut p, 65);
        assert!(a > 0.2 && (a - gm).abs() < 1e-6, "C5 and GM C3 both play pad 1");
        assert!((f / a - 0.5).abs() < 0.01, "pad 6 plays its own, quieter sample");
        assert_eq!(hit(&mut p, 61), 0.0, "pad 2 has no sample");
        assert_eq!(pad_for_key(80), None);
        assert!(p.set_sample_at(16, Some(click(1.0, 10))).is_some(), "out-of-range pad hands the sample back");
    }

    #[test]
    fn params_are_unique_and_named_per_pad() {
        assert_eq!(PARAMS[volume_id(15) as usize].key, "pad16.volume");
        assert_eq!(PARAMS[tune_id(0) as usize].key, "pad1.tune");
        assert_eq!(PARAMS[MASTER as usize].key, "master.volume");
    }
}
