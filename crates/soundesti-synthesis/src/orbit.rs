//! Orbit — two-operator FM synthesizer (modulator → carrier, with feedback and an
//! index envelope). Covers the bells, e-pianos, basses and plucks role of a classic FM
//! synth. Original design and code.

use soundesti_dsp::{Adsr, AdsrParams, midi_to_freq};
use soundesti_plugin_api::{Instrument, NoteEvent, NoteId, ParamInfo, TimedEvent, for_each_segment};

use crate::common::{ParamStore, cont, decay_coef, pick_voice};

pub const TYPE_ID: &str = "soundesti.orbit";
const VOICES: usize = 16;

pub mod param {
    pub const RATIO: u32 = 0;
    pub const INDEX: u32 = 1;
    pub const INDEX_DECAY: u32 = 2;
    pub const INDEX_SUSTAIN: u32 = 3;
    pub const FEEDBACK: u32 = 4;
    pub const ATTACK: u32 = 5;
    pub const DECAY: u32 = 6;
    pub const SUSTAIN: u32 = 7;
    pub const RELEASE: u32 = 8;
    pub const VOLUME: u32 = 9;
}

pub static PARAMS: [ParamInfo; 10] = [
    cont(param::RATIO, "mod.ratio", "Ratio", "Modulator", 0.25, 12.0, 2.0, ""),
    cont(param::INDEX, "mod.index", "Amount", "Modulator", 0.0, 10.0, 3.0, ""),
    cont(param::INDEX_DECAY, "mod.decay", "Amount decay", "Modulator", 0.01, 5.0, 0.6, "s"),
    cont(param::INDEX_SUSTAIN, "mod.sustain", "Amount sustain", "Modulator", 0.0, 1.0, 0.2, ""),
    cont(param::FEEDBACK, "mod.feedback", "Feedback", "Modulator", 0.0, 1.0, 0.0, ""),
    cont(param::ATTACK, "amp.attack", "Attack", "Envelope", 0.0, 5.0, 0.002, "s"),
    cont(param::DECAY, "amp.decay", "Decay", "Envelope", 0.0, 5.0, 1.2, "s"),
    cont(param::SUSTAIN, "amp.sustain", "Sustain", "Envelope", 0.0, 1.0, 0.3, ""),
    cont(param::RELEASE, "amp.release", "Release", "Envelope", 0.0, 5.0, 0.4, "s"),
    cont(param::VOLUME, "master.volume", "Volume", "Master", 0.0, 1.0, 0.7, ""),
];

struct Voice {
    env: Adsr,
    pc: f32,
    pm: f32,
    last_mod: f32,
    index_env: f32,
    freq: f32,
    gain: f32,
    vel: f32,
    id: NoteId,
    gate: bool,
    age: u64,
}

pub struct Orbit {
    sr: f32,
    p: ParamStore<10>,
    voices: Vec<Voice>,
    age: u64,
}

impl Orbit {
    pub fn new(sample_rate: f32) -> Self {
        let voices = (0..VOICES)
            .map(|_| Voice {
                env: Adsr::new(sample_rate),
                pc: 0.0,
                pm: 0.0,
                last_mod: 0.0,
                index_env: 0.0,
                freq: 440.0,
                gain: 0.0,
                vel: 0.0,
                id: 0,
                gate: false,
                age: 0,
            })
            .collect();
        Self { sr: sample_rate, p: ParamStore::new(&PARAMS), voices, age: 1 }
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
        let ratio = self.p.get(param::RATIO);
        let index = self.p.get(param::INDEX);
        let isus = self.p.get(param::INDEX_SUSTAIN);
        let fb = self.p.get(param::FEEDBACK);
        let vol = self.p.get(param::VOLUME) * 0.35;
        let icoef = decay_coef(self.p.get(param::INDEX_DECAY), self.sr);
        let tau = std::f32::consts::TAU;
        for v in self.voices.iter_mut().filter(|v| !v.env.is_idle()) {
            let (inc_c, inc_m) = (v.freq / self.sr, v.freq * ratio / self.sr);
            for i in range.clone() {
                let m = (tau * v.pm + fb * 2.0 * v.last_mod).sin();
                v.last_mod = m;
                let idx = index * (0.4 + 0.6 * v.vel) * (isus + (1.0 - isus) * v.index_env);
                let y = (tau * v.pc + idx * m).sin() * v.env.next() * v.gain * vol;
                v.pc = (v.pc + inc_c).fract();
                v.pm = (v.pm + inc_m).fract();
                v.index_env *= icoef;
                l[i] += y;
                r[i] += y;
                if v.env.is_idle() {
                    break;
                }
            }
        }
    }

    fn handle(&mut self, ev: NoteEvent) {
        match ev {
            NoteEvent::On { id, key, velocity } => {
                let i = pick_voice(&self.voices, |v| v.env.is_idle(), |v| v.age);
                self.age += 1;
                let adsr = self.adsr();
                let v = &mut self.voices[i];
                if v.env.is_idle() {
                    v.pc = 0.0;
                    v.pm = 0.0;
                    v.last_mod = 0.0;
                }
                v.env.set_params(adsr);
                v.env.gate_on();
                v.freq = midi_to_freq(key as f32);
                v.vel = velocity.clamp(0.0, 1.0);
                v.gain = 0.3 + 0.7 * v.vel;
                v.index_env = 1.0;
                v.id = id;
                v.gate = true;
                v.age = self.age;
            }
            NoteEvent::Off { id } => {
                for v in self.voices.iter_mut().filter(|v| v.gate && v.id == id) {
                    v.gate = false;
                    v.env.gate_off();
                }
            }
            NoteEvent::AllNotesOff => self.voices.iter_mut().for_each(|v| {
                v.gate = false;
                v.env.gate_off()
            }),
            NoteEvent::AllSoundOff => self.voices.iter_mut().for_each(|v| {
                v.gate = false;
                v.env.kill()
            }),
        }
    }
}

impl Instrument for Orbit {
    fn type_id(&self) -> &'static str {
        TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Orbit"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &PARAMS
    }
    fn get_param(&self, id: u32) -> f32 {
        self.p.get(id)
    }
    fn set_param(&mut self, id: u32, value: f32) {
        self.p.set(id, value);
        let adsr = self.adsr();
        for v in self.voices.iter_mut().filter(|v| !v.env.is_idle()) {
            v.env.set_params(adsr);
        }
    }
    fn process(&mut self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut ctx = (self, left, right);
        for_each_segment(&mut ctx, events, n, |(s, _, _), ev| s.handle(ev), |(s, l, r), range| s.render(range, l, r));
    }
    fn active_voices(&self) -> usize {
        self.voices.iter().filter(|v| !v.env.is_idle()).count()
    }
}
