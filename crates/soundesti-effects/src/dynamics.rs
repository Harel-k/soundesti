//! Compressor and peak limiter (stereo-linked).

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont, db};

pub const COMP_ID: &str = "soundesti.compressor";
pub const LIMIT_ID: &str = "soundesti.limiter";

pub mod cp {
    pub const THRESHOLD: u32 = 0;
    pub const RATIO: u32 = 1;
    pub const KNEE: u32 = 2;
    pub const ATTACK: u32 = 3;
    pub const RELEASE: u32 = 4;
    pub const MAKEUP: u32 = 5;
    pub const MIX: u32 = 6;
}

pub static COMP_PARAMS: [ParamInfo; 7] = [
    cont(cp::THRESHOLD, "comp.threshold", "Threshold", "Compressor", -60.0, 0.0, -18.0, "dB"),
    cont(cp::RATIO, "comp.ratio", "Ratio", "Compressor", 1.0, 20.0, 4.0, ":1"),
    cont(cp::KNEE, "comp.knee", "Knee", "Compressor", 0.0, 12.0, 3.0, "dB"),
    cont(cp::ATTACK, "comp.attack", "Attack", "Timing", 0.1, 100.0, 10.0, "ms"),
    cont(cp::RELEASE, "comp.release", "Release", "Timing", 10.0, 1000.0, 120.0, "ms"),
    cont(cp::MAKEUP, "comp.makeup", "Makeup", "Output", -12.0, 24.0, 0.0, "dB"),
    cont(cp::MIX, "comp.mix", "Mix", "Output", 0.0, 1.0, 1.0, ""),
];

fn coef(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms.max(0.01) * 0.001 * sr)).exp()
}

pub struct Compressor {
    sr: f32,
    p: Params<7>,
    /// Smoothed gain reduction in dB (<= 0).
    gr: f32,
}

impl Compressor {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&COMP_PARAMS), gr: 0.0 }
    }

    /// Static curve: gain change in dB for an input level in dB (soft knee).
    pub fn curve(level: f32, threshold: f32, ratio: f32, knee: f32) -> f32 {
        let over = level - threshold;
        if knee > 0.0 && over.abs() <= knee / 2.0 {
            let x = over + knee / 2.0;
            (1.0 / ratio - 1.0) * x * x / (2.0 * knee)
        } else if over > 0.0 {
            (1.0 / ratio - 1.0) * over
        } else {
            0.0
        }
    }
}

impl Effect for Compressor {
    crate::effect_common!(COMP_ID, "Compressor", COMP_PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let (th, ratio, knee) = (self.p.get(cp::THRESHOLD), self.p.get(cp::RATIO), self.p.get(cp::KNEE));
        let (att, rel) = (coef(self.p.get(cp::ATTACK), self.sr), coef(self.p.get(cp::RELEASE), self.sr));
        let (makeup, mix) = (db(self.p.get(cp::MAKEUP)), self.p.get(cp::MIX));
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let peak = l.abs().max(r.abs()).max(1e-9);
            let target = Self::curve(20.0 * peak.log10(), th, ratio, knee);
            // Attack when more reduction is needed, release when less.
            let c = if target < self.gr { att } else { rel };
            self.gr = target + (self.gr - target) * c;
            let g = db(self.gr) * makeup;
            *l += (*l * g - *l) * mix;
            *r += (*r * g - *r) * mix;
        }
    }

    fn reset(&mut self) {
        self.gr = 0.0;
    }
}

pub mod lp {
    pub const INPUT: u32 = 0;
    pub const CEILING: u32 = 1;
    pub const RELEASE: u32 = 2;
}

pub static LIMIT_PARAMS: [ParamInfo; 3] = [
    cont(lp::INPUT, "limit.input", "Input gain", "Limiter", 0.0, 24.0, 0.0, "dB"),
    cont(lp::CEILING, "limit.ceiling", "Ceiling", "Limiter", -12.0, 0.0, -0.3, "dB"),
    cont(lp::RELEASE, "limit.release", "Release", "Limiter", 5.0, 500.0, 60.0, "ms"),
];

pub struct Limiter {
    sr: f32,
    p: Params<3>,
    gain: f32,
}

impl Limiter {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&LIMIT_PARAMS), gain: 1.0 }
    }
}

impl Effect for Limiter {
    crate::effect_common!(LIMIT_ID, "Limiter", LIMIT_PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let input = db(self.p.get(lp::INPUT));
        let ceiling = db(self.p.get(lp::CEILING));
        let rel = coef(self.p.get(lp::RELEASE), self.sr);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (xl, xr) = (*l * input, *r * input);
            let peak = xl.abs().max(xr.abs());
            let needed = if peak > ceiling { ceiling / peak } else { 1.0 };
            // Instant attack, smooth release.
            self.gain = if needed < self.gain { needed } else { needed + (self.gain - needed) * rel };
            // Final safety clip so the ceiling is never exceeded.
            *l = (xl * self.gain).clamp(-ceiling, ceiling);
            *r = (xr * self.gain).clamp(-ceiling, ceiling);
        }
    }

    fn reset(&mut self) {
        self.gain = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testutil::sine_gain;

    #[test]
    fn compressor_reduces_loud_signals_only() {
        let sr = 48_000.0;
        let mut c = Compressor::new(sr);
        c.set_param(cp::THRESHOLD, -30.0);
        c.set_param(cp::RATIO, 10.0);
        c.set_param(cp::KNEE, 0.0);
        // Test sine peaks at 0.25 (about -12 dB): 18 dB over, 10:1 -> about 16 dB of reduction.
        let g = sine_gain(|l, r| c.process(l, r), 200.0, sr);
        assert!(g < 0.25, "compressor barely reduced: x{g}");
        let mut c = Compressor::new(sr);
        c.set_param(cp::THRESHOLD, 0.0);
        assert!((sine_gain(|l, r| c.process(l, r), 200.0, sr) - 1.0).abs() < 0.05, "below threshold must be untouched");
    }

    #[test]
    fn soft_knee_curve_is_continuous() {
        let a = Compressor::curve(-18.0 + 1.5, -18.0, 4.0, 3.0);
        let b = Compressor::curve(-18.0 + 1.5001, -18.0, 4.0, 3.0);
        assert!((a - b).abs() < 0.01);
    }

    #[test]
    fn limiter_never_exceeds_ceiling() {
        let mut lim = Limiter::new(48_000.0);
        lim.set_param(lp::INPUT, 18.0);
        let mut l: Vec<f32> = (0..48_000).map(|i| (i as f32 * 0.05).sin()).collect();
        let mut r = l.clone();
        lim.process(&mut l, &mut r);
        let ceiling = db(-0.3);
        assert!(l.iter().chain(r.iter()).all(|x| x.abs() <= ceiling + 1e-6));
    }
}
