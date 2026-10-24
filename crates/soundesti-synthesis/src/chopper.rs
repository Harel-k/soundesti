//! Chopper — loop slicer (the role of a beat slicer). Cuts a sample into equal slices or
//! at detected hits, and maps slice 1 to the base key, slice 2 to the next key, and so
//! on, so a loop can be replayed in any order from the piano roll. Original code.

use std::sync::Arc;

use soundesti_plugin_api::{Instrument, NoteEvent, NoteId, ParamInfo, SampleData, TimedEvent, for_each_segment};

use crate::common::{ParamStore, cont, pick_voice, semis, stepped};

pub const TYPE_ID: &str = "soundesti.chopper";
const VOICES: usize = 16;
const MAX_SLICES: usize = 64;

pub mod param {
    pub const SLICES: u32 = 0;
    pub const MODE: u32 = 1;
    pub const BASE: u32 = 2;
    pub const TUNE: u32 = 3;
    pub const FADE: u32 = 4;
    pub const GATED: u32 = 5;
    pub const VOLUME: u32 = 6;
}

const SLICE_COUNTS: [usize; 4] = [4, 8, 16, 32];

pub static PARAMS: [ParamInfo; 7] = [
    stepped(param::SLICES, "chop.slices", "Slices", "Slicing", 0.0, 3.0, 2.0, &["4", "8", "16", "32"]),
    stepped(param::MODE, "chop.mode", "Cut at", "Slicing", 0.0, 1.0, 0.0, &["Equal parts", "Detected hits"]),
    stepped(param::BASE, "chop.base", "First slice key", "Slicing", 0.0, 127.0, 60.0, &[]),
    cont(param::TUNE, "chop.tune", "Tune", "Playback", -24.0, 24.0, 0.0, "st"),
    cont(param::FADE, "chop.fade", "Edge fade", "Playback", 0.0, 50.0, 3.0, "ms"),
    stepped(param::GATED, "chop.gated", "Playback", "Playback", 0.0, 1.0, 0.0, &["Whole slice", "While held"]),
    cont(param::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.8, ""),
];

struct Voice {
    pos: f64,
    end: f64,
    start: f64,
    step: f64,
    gain: f32,
    release: f32,
    id: NoteId,
    gate: bool,
    active: bool,
    age: u64,
}

pub struct Chopper {
    sr: f32,
    p: ParamStore<7>,
    sample: Option<Arc<SampleData>>,
    starts: [usize; MAX_SLICES + 1],
    count: usize,
    /// Slicing settings the current `starts` were computed for.
    sliced_for: (usize, usize, usize),
    voices: Vec<Voice>,
    age: u64,
}

impl Chopper {
    pub fn new(sample_rate: f32) -> Self {
        let voices = (0..VOICES)
            .map(|_| Voice {
                pos: 0.0,
                end: 0.0,
                start: 0.0,
                step: 1.0,
                gain: 0.0,
                release: 1.0,
                id: 0,
                gate: false,
                active: false,
                age: 0,
            })
            .collect();
        Self {
            sr: sample_rate,
            p: ParamStore::new(&PARAMS),
            sample: None,
            starts: [0; MAX_SLICES + 1],
            count: 0,
            sliced_for: (0, 0, 0),
            voices,
            age: 1,
        }
    }

    /// Current slice boundaries as sample indices (count + 1 entries).
    pub fn slice_points(&self) -> &[usize] {
        &self.starts[..self.count + 1]
    }

    fn reslice(&mut self) {
        let Some(s) = self.sample.as_ref() else {
            self.count = 0;
            return;
        };
        let len = s.len();
        let want = SLICE_COUNTS[(self.p.get(param::SLICES) as usize).min(3)];
        let mode = self.p.get(param::MODE) as usize;
        if self.sliced_for == (len, want, mode) && self.count > 0 {
            return;
        }
        self.sliced_for = (len, want, mode);
        if mode == 1 {
            self.count = detect_hits(s, want, &mut self.starts);
        } else {
            self.count = want;
            for (i, st) in self.starts.iter_mut().enumerate().take(want + 1) {
                *st = len * i / want;
            }
        }
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let Some(s) = self.sample.as_ref() else { return };
        let vol = self.p.get(param::VOLUME);
        let fade = (self.p.get(param::FADE) * 0.001 * s.sample_rate).max(1.0) as f64;
        let rel_step = 1.0 / (0.01 * self.sr);
        for v in self.voices.iter_mut().filter(|v| v.active) {
            for i in range.clone() {
                if v.pos + 1.0 >= v.end || v.release <= 0.0 {
                    v.active = false;
                    break;
                }
                let idx = v.pos as usize;
                let frac = (v.pos - idx as f64) as f32;
                let sl = s.left[idx] + (s.left[idx + 1] - s.left[idx]) * frac;
                let sr = s.right[idx] + (s.right[idx + 1] - s.right[idx]) * frac;
                // Short fades at both slice edges avoid clicks where the cut lands mid-wave.
                let edge = ((v.pos - v.start) / fade).min((v.end - v.pos) / fade).clamp(0.0, 1.0) as f32;
                if !v.gate && self.p.get(param::GATED) > 0.5 {
                    v.release -= rel_step;
                }
                let g = v.gain * vol * edge * v.release.max(0.0);
                l[i] += sl * g;
                r[i] += sr * g;
                v.pos += v.step;
            }
        }
    }

    fn handle(&mut self, ev: NoteEvent) {
        match ev {
            NoteEvent::On { id, key, velocity } => {
                self.reslice();
                let Some(s) = self.sample.as_ref() else { return };
                if self.count == 0 {
                    return;
                }
                let base = self.p.get(param::BASE) as i32;
                let slice = (key as i32 - base).rem_euclid(self.count as i32) as usize;
                let (a, b) = (self.starts[slice], self.starts[slice + 1].min(s.len()));
                if b <= a + 2 {
                    return;
                }
                let step = (s.sample_rate / self.sr) as f64 * semis(self.p.get(param::TUNE)) as f64;
                let i = pick_voice(&self.voices, |v| !v.active, |v| v.age);
                self.age += 1;
                self.voices[i] = Voice {
                    pos: a as f64,
                    start: a as f64,
                    end: b as f64,
                    step,
                    gain: velocity.clamp(0.0, 1.0),
                    release: 1.0,
                    id,
                    gate: true,
                    active: true,
                    age: self.age,
                };
            }
            NoteEvent::Off { id } => {
                for v in self.voices.iter_mut().filter(|v| v.id == id) {
                    v.gate = false;
                }
            }
            NoteEvent::AllNotesOff => self.voices.iter_mut().for_each(|v| v.gate = false),
            NoteEvent::AllSoundOff => self.voices.iter_mut().for_each(|v| v.active = false),
        }
    }
}

/// Up to `want` slice starts at the strongest energy rises (always including 0).
/// Writes `count + 1` boundaries into `out` and returns `count`. No allocation.
fn detect_hits(s: &SampleData, want: usize, out: &mut [usize; MAX_SLICES + 1]) -> usize {
    let len = s.len();
    const WIN: usize = 512;
    let windows = len / WIN;
    if windows < 2 {
        out[0] = 0;
        out[1] = len;
        return 1;
    }
    // Keep the `want` largest rises in a small fixed list.
    let mut best: [(f32, usize); MAX_SLICES] = [(0.0, 0); MAX_SLICES];
    let mut prev = 0.0f32;
    let min_gap = windows / (want * 2).max(1);
    for w in 0..windows {
        let e: f32 =
            (w * WIN..(w + 1) * WIN).map(|i| s.left[i] * s.left[i] + s.right[i] * s.right[i]).sum::<f32>() / WIN as f32;
        let rise = (e - prev * 1.5).max(0.0);
        prev = e;
        if w == 0 || rise <= 0.0 {
            continue;
        }
        let pos = w * WIN;
        // Replace the weakest entry if this rise is stronger and not too close to a kept one.
        if best.iter().take(want).any(|&(r, p)| r > 0.0 && p.abs_diff(pos) < min_gap * WIN) {
            if let Some(slot) = best.iter_mut().take(want).find(|(r, p)| *r < rise && p.abs_diff(pos) < min_gap * WIN) {
                *slot = (rise, pos);
            }
            continue;
        }
        let (wi, _) = best.iter().take(want).enumerate().min_by(|a, b| a.1.0.total_cmp(&b.1.0)).unwrap();
        if best[wi].0 < rise {
            best[wi] = (rise, pos);
        }
    }
    let mut n = 0;
    out[0] = 0;
    n += 1;
    let mut picked: [usize; MAX_SLICES] = [0; MAX_SLICES];
    let mut k = 0;
    for &(r, p) in best.iter().take(want) {
        if r > 0.0 {
            picked[k] = p;
            k += 1;
        }
    }
    picked[..k].sort_unstable();
    for &p in &picked[..k] {
        if n < want && p > out[n - 1] + WIN {
            out[n] = p;
            n += 1;
        }
    }
    out[n] = len;
    n
}

impl Instrument for Chopper {
    fn type_id(&self) -> &'static str {
        TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Chopper"
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
        for v in &mut self.voices {
            v.active = false;
        }
        let old = std::mem::replace(&mut self.sample, sample);
        self.count = 0;
        self.reslice();
        old
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Four hits: silence with a burst at the start of each quarter.
    fn loop_with_hits(sr: f32) -> Arc<SampleData> {
        let n = sr as usize;
        let v: Vec<f32> = (0..n)
            .map(|i| {
                if i % (n / 4) < 2_000 {
                    ((i as f32) * 0.3).sin() * (1.0 - (i % (n / 4)) as f32 / 2_000.0)
                } else {
                    0.0
                }
            })
            .collect();
        Arc::new(SampleData { left: v.clone(), right: v, sample_rate: sr, name: "loop".into() })
    }

    #[test]
    fn keys_play_their_slices() {
        let sr = 48_000.0;
        let mut c = Chopper::new(sr);
        c.set_param(param::SLICES, 0.0); // 4 slices
        c.set_sample(Some(loop_with_hits(sr)));
        assert_eq!(c.slice_points(), [0, 12_000, 24_000, 36_000, 48_000]);
        // Key 62 = third slice: it starts with a hit and lasts a quarter second.
        let (mut l, mut r) = (vec![0.0; 14_000], vec![0.0; 14_000]);
        c.process(&[TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key: 62, velocity: 1.0 } }], &mut l, &mut r);
        assert!(l[..1_000].iter().any(|x| x.abs() > 0.1));
        assert!(l[12_100..].iter().all(|x| *x == 0.0), "stops at the slice end");
    }

    #[test]
    fn hit_detection_finds_the_bursts() {
        let sr = 48_000.0;
        let mut c = Chopper::new(sr);
        c.set_param(param::MODE, 1.0);
        c.set_param(param::SLICES, 0.0);
        c.set_sample(Some(loop_with_hits(sr)));
        let pts = c.slice_points().to_vec();
        assert_eq!(pts.len(), 5, "{pts:?}");
        for (p, want) in pts.iter().zip([0, 12_000, 24_000, 36_000]) {
            assert!(p.abs_diff(want) <= 512, "slice at {p}, expected near {want}");
        }
    }
}
