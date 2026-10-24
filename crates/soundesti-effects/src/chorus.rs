//! Stereo chorus: two LFO-modulated delay taps in quadrature.

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont};

pub const TYPE_ID: &str = "soundesti.chorus";
const MAX_MS: f32 = 40.0;

pub mod p {
    pub const RATE: u32 = 0;
    pub const DEPTH: u32 = 1;
    pub const DELAY: u32 = 2;
    pub const MIX: u32 = 3;
}

pub static PARAMS: [ParamInfo; 4] = [
    cont(p::RATE, "chorus.rate", "Rate", "Chorus", 0.05, 5.0, 0.8, "Hz"),
    cont(p::DEPTH, "chorus.depth", "Depth", "Chorus", 0.0, 1.0, 0.5, ""),
    cont(p::DELAY, "chorus.delay", "Delay", "Chorus", 5.0, 25.0, 12.0, "ms"),
    cont(p::MIX, "chorus.mix", "Mix", "Output", 0.0, 1.0, 0.5, ""),
];

pub struct Chorus {
    sr: f32,
    p: Params<4>,
    buf: [Vec<f32>; 2],
    pos: usize,
    phase: f32,
}

impl Chorus {
    pub fn new(sr: f32) -> Self {
        let n = (sr * MAX_MS / 1000.0) as usize + 4;
        Self { sr, p: Params::new(&PARAMS), buf: [vec![0.0; n], vec![0.0; n]], pos: 0, phase: 0.0 }
    }
}

impl Effect for Chorus {
    crate::effect_common!(TYPE_ID, "Chorus", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = self.buf[0].len();
        let rate = self.p.get(p::RATE) / self.sr;
        let base = self.p.get(p::DELAY) / 1000.0 * self.sr;
        let depth = self.p.get(p::DEPTH) * 0.006 * self.sr;
        let mix = self.p.get(p::MIX);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            self.buf[0][self.pos] = *l;
            self.buf[1][self.pos] = *r;
            let mut wet = [0.0f32; 2];
            for (ch, w) in wet.iter_mut().enumerate() {
                let lfo = ((self.phase + ch as f32 * 0.25) * std::f32::consts::TAU).sin();
                let d = (base + depth * (0.5 + 0.5 * lfo)).clamp(1.0, (n - 2) as f32);
                let rp = self.pos as f32 - d + n as f32;
                let i0 = rp as usize % n;
                let frac = rp.fract();
                *w = self.buf[ch][i0] * (1.0 - frac) + self.buf[ch][(i0 + 1) % n] * frac;
            }
            self.pos = (self.pos + 1) % n;
            self.phase = (self.phase + rate).fract();
            *l = *l * (1.0 - mix * 0.5) + wet[0] * mix;
            *r = *r * (1.0 - mix * 0.5) + wet[1] * mix;
        }
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
    }
}
