//! Harpa — plucked-string physical model (extended Karplus-Strong): a noise burst
//! excites a tuned delay line with a damping filter in the feedback loop. Covers the
//! guitar / harp / pluck role. Original code; the Karplus-Strong algorithm is public.

use soundesti_dsp::{Noise, midi_to_freq};
use soundesti_plugin_api::{Instrument, NoteEvent, NoteId, ParamInfo, TimedEvent, for_each_segment};

use crate::common::{ParamStore, cont, pick_voice};

pub const TYPE_ID: &str = "soundesti.harpa";
const VOICES: usize = 12;
const MIN_FREQ: f32 = 25.0;

pub mod param {
    pub const SUSTAIN: u32 = 0;
    pub const BRIGHTNESS: u32 = 1;
    pub const PICK: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const VOLUME: u32 = 4;
}

pub static PARAMS: [ParamInfo; 5] = [
    cont(param::SUSTAIN, "string.sustain", "Sustain", "String", 0.2, 10.0, 2.5, "s"),
    cont(param::BRIGHTNESS, "string.brightness", "Brightness", "String", 0.0, 1.0, 0.55, ""),
    cont(param::PICK, "string.pick", "Pick hardness", "String", 0.0, 1.0, 0.7, ""),
    cont(param::RELEASE, "string.release", "Release", "String", 0.02, 2.0, 0.25, "s"),
    cont(param::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.8, ""),
];

struct Voice {
    line: Vec<f32>,
    write: usize,
    period: f32,
    last: f32,
    gain: f32,
    loop_gain: f32,
    damping: f32,
    level: f32,
    id: NoteId,
    gate: bool,
    active: bool,
    age: u64,
}

pub struct Harpa {
    sr: f32,
    p: ParamStore<5>,
    voices: Vec<Voice>,
    noise: Noise,
    age: u64,
}

impl Harpa {
    pub fn new(sample_rate: f32) -> Self {
        // Delay lines are sized for the lowest playable note and allocated up front.
        let len = (sample_rate / MIN_FREQ) as usize + 4;
        let voices = (0..VOICES)
            .map(|_| Voice {
                line: vec![0.0; len],
                write: 0,
                period: 100.0,
                last: 0.0,
                gain: 0.0,
                loop_gain: 0.99,
                damping: 1.0,
                level: 0.0,
                id: 0,
                gate: false,
                active: false,
                age: 0,
            })
            .collect();
        Self { sr: sample_rate, p: ParamStore::new(&PARAMS), voices, noise: Noise::new(31337), age: 1 }
    }

    fn loop_gain(&self, period: f32) -> f32 {
        // Gain per trip around the loop so the string falls 60 dB in `sustain` seconds.
        0.001f32.powf(period / (self.p.get(param::SUSTAIN) * self.sr))
    }

    fn render(&mut self, range: std::ops::Range<usize>, l: &mut [f32], r: &mut [f32]) {
        let bright = self.p.get(param::BRIGHTNESS);
        let vol = self.p.get(param::VOLUME) * 0.6;
        let release_gain = 0.001f32.powf(1.0 / (self.p.get(param::RELEASE) * self.sr));
        // One-pole lowpass in the loop: more brightness = less smoothing.
        let lp = 0.15 + 0.85 * bright;
        for v in self.voices.iter_mut().filter(|v| v.active) {
            let len = v.line.len();
            for i in range.clone() {
                let read_pos = v.write as f32 - v.period;
                let read_pos = if read_pos < 0.0 { read_pos + len as f32 } else { read_pos };
                let i0 = read_pos as usize % len;
                let i1 = (i0 + 1) % len;
                let frac = read_pos.fract();
                let delayed = v.line[i0] * (1.0 - frac) + v.line[i1] * frac;
                let filtered = v.last + lp * (delayed - v.last);
                v.last = filtered;
                let fb = filtered * v.loop_gain * v.damping;
                v.line[v.write] = fb;
                v.write = (v.write + 1) % len;
                let y = delayed * v.gain * vol;
                l[i] += y;
                r[i] += y;
                if !v.gate {
                    v.damping *= release_gain;
                }
                v.level = v.level * 0.999 + delayed.abs() * 0.001;
                if v.level < 1e-5 || v.damping < 1e-3 {
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
                let freq = midi_to_freq(key as f32).max(MIN_FREQ);
                let period = self.sr / freq;
                let loop_gain = self.loop_gain(period);
                let pick = self.p.get(param::PICK);
                let vel = velocity.clamp(0.0, 1.0);
                let v = &mut self.voices[i];
                v.line.fill(0.0);
                // Excitation: a period of noise, softened for gentle picks.
                let n = (period as usize).min(v.line.len() - 1);
                let mut prev = 0.0;
                for k in 0..n {
                    let x = self.noise.next();
                    prev = prev + (0.2 + 0.8 * pick) * (x - prev);
                    v.line[k] = prev;
                }
                v.write = n;
                v.period = period;
                v.loop_gain = loop_gain;
                v.last = 0.0;
                v.damping = 1.0;
                v.level = 1.0;
                v.gain = 0.3 + 0.7 * vel;
                v.id = id;
                v.gate = true;
                v.active = true;
                v.age = self.age;
            }
            NoteEvent::Off { id } => {
                self.voices.iter_mut().filter(|v| v.gate && v.id == id).for_each(|v| v.gate = false)
            }
            NoteEvent::AllNotesOff => self.voices.iter_mut().for_each(|v| v.gate = false),
            NoteEvent::AllSoundOff => self.voices.iter_mut().for_each(|v| {
                v.gate = false;
                v.active = false;
            }),
        }
    }
}

impl Instrument for Harpa {
    fn type_id(&self) -> &'static str {
        TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Harpa"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &PARAMS
    }
    fn get_param(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set_param(&mut self, id: u32, value: f32) {
        self.p.set(id, value);
        for vi in 0..self.voices.len() {
            if self.voices[vi].active {
                let g = self.loop_gain(self.voices[vi].period);
                self.voices[vi].loop_gain = g;
            }
        }
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
