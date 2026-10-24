//! Algorithmic reverb: parallel damped comb filters into series allpasses per channel
//! (the classic Schroeder/Moorer topology, as popularized by the public-domain Freeverb).
//! Original implementation.

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont};

pub const TYPE_ID: &str = "soundesti.reverb";

pub mod p {
    pub const SIZE: u32 = 0;
    pub const DAMPING: u32 = 1;
    pub const WIDTH: u32 = 2;
    pub const PREDELAY: u32 = 3;
    pub const MIX: u32 = 4;
}

pub static PARAMS: [ParamInfo; 5] = [
    cont(p::SIZE, "reverb.size", "Room size", "Room", 0.0, 1.0, 0.65, ""),
    cont(p::DAMPING, "reverb.damping", "Damping", "Room", 0.0, 1.0, 0.45, ""),
    cont(p::WIDTH, "reverb.width", "Width", "Room", 0.0, 1.0, 1.0, ""),
    cont(p::PREDELAY, "reverb.predelay", "Pre-delay", "Room", 0.0, 200.0, 12.0, "ms"),
    cont(p::MIX, "reverb.mix", "Mix", "Output", 0.0, 1.0, 0.25, ""),
];

const COMBS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASSES: [usize; 4] = [556, 441, 341, 225];
const SPREAD: usize = 23;

struct Comb {
    buf: Vec<f32>,
    pos: usize,
    store: f32,
}

impl Comb {
    #[inline]
    fn process(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let y = self.buf[self.pos];
        self.store = y * (1.0 - damp) + self.store * damp;
        self.buf[self.pos] = x + self.store * feedback;
        self.pos = (self.pos + 1) % self.buf.len();
        y
    }
}

struct Allpass {
    buf: Vec<f32>,
    pos: usize,
}

impl Allpass {
    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let b = self.buf[self.pos];
        let y = b - x;
        self.buf[self.pos] = x + b * 0.5;
        self.pos = (self.pos + 1) % self.buf.len();
        y
    }
}

pub struct Reverb {
    sr: f32,
    p: Params<5>,
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
    pre: Vec<f32>,
    pre_pos: usize,
}

impl Reverb {
    pub fn new(sr: f32) -> Self {
        let scale = sr / 44_100.0;
        let mk = |n: usize, extra: usize| ((n + extra) as f32 * scale) as usize + 1;
        let combs =
            [0, SPREAD].map(|s| COMBS.iter().map(|&n| Comb { buf: vec![0.0; mk(n, s)], pos: 0, store: 0.0 }).collect());
        let allpasses =
            [0, SPREAD].map(|s| ALLPASSES.iter().map(|&n| Allpass { buf: vec![0.0; mk(n, s)], pos: 0 }).collect());
        Self { sr, p: Params::new(&PARAMS), combs, allpasses, pre: vec![0.0; (sr * 0.2) as usize + 2], pre_pos: 0 }
    }
}

impl Effect for Reverb {
    crate::effect_common!(TYPE_ID, "Reverb", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let feedback = 0.7 + 0.28 * self.p.get(p::SIZE);
        let damp = 0.4 * self.p.get(p::DAMPING);
        let width = self.p.get(p::WIDTH);
        let mix = self.p.get(p::MIX);
        let pre_len = ((self.p.get(p::PREDELAY) / 1000.0 * self.sr) as usize).min(self.pre.len() - 1);
        let (w1, w2) = (0.5 + width / 2.0, (1.0 - width) / 2.0);
        let n_pre = self.pre.len();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            // Mono send through the pre-delay, scaled down like a reverb input stage.
            self.pre[self.pre_pos] = (*l + *r) * 0.015;
            let input = self.pre[(self.pre_pos + n_pre - pre_len) % n_pre];
            self.pre_pos = (self.pre_pos + 1) % n_pre;
            let mut out = [0.0f32; 2];
            for ((o, combs), allpasses) in out.iter_mut().zip(&mut self.combs).zip(&mut self.allpasses) {
                let mut acc = 0.0;
                for c in combs {
                    acc += c.process(input, feedback, damp);
                }
                for a in allpasses {
                    acc = a.process(acc);
                }
                *o = acc;
            }
            let wl = out[0] * w1 + out[1] * w2;
            let wr = out[1] * w1 + out[0] * w2;
            *l += wl * mix * 3.0;
            *r += wr * mix * 3.0;
        }
    }

    fn reset(&mut self) {
        for c in self.combs.iter_mut().flatten() {
            c.buf.fill(0.0);
            c.store = 0.0;
        }
        for a in self.allpasses.iter_mut().flatten() {
            a.buf.fill(0.0);
        }
        self.pre.fill(0.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn impulse_has_a_decaying_tail() {
        let mut rv = Reverb::new(48_000.0);
        rv.set_param(p::MIX, 1.0);
        let mut l = vec![0.0; 96_000 * 3];
        let mut r = vec![0.0; 96_000 * 3];
        l[0] = 1.0;
        r[0] = 1.0;
        rv.process(&mut l, &mut r);
        let energy = |a: usize, b: usize| l[a..b].iter().map(|x| x * x).sum::<f32>();
        let early = energy(2_000, 26_000);
        let late = energy(200_000, 224_000);
        assert!(early > 1e-6, "no reverb tail");
        assert!(late < early * 0.05, "tail must decay");
    }
}
