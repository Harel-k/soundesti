//! Three-band compressor (Linkwitz-Riley 4th-order crossovers) and "Polish", a one-knob
//! enhancer built from the same bands plus gentle saturation and air (the role of a
//! "make it sound better" mastering effect).

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Biquad, Params, Shape, cont, db};
use crate::dynamics::Compressor;

pub const MULTIBAND_ID: &str = "soundesti.multiband";
pub const POLISH_ID: &str = "soundesti.polish";

/// LR4 split: two cascaded Butterworth sections per side.
#[derive(Clone, Copy, Default)]
struct Crossover {
    lp: [Biquad; 2],
    hp: [Biquad; 2],
}

impl Crossover {
    fn set(&mut self, freq: f32, sr: f32) {
        for b in &mut self.lp {
            b.set(Shape::LowPass, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, sr);
        }
        for b in &mut self.hp {
            b.set(Shape::HighPass, freq, std::f32::consts::FRAC_1_SQRT_2, 0.0, sr);
        }
    }

    fn split(&mut self, x: f32) -> (f32, f32) {
        let lo = self.lp[0].process(x);
        let hi = self.hp[0].process(x);
        (self.lp[1].process(lo), self.hp[1].process(hi))
    }
}

/// Per-band envelope follower + compressor curve (stereo-linked).
#[derive(Clone, Copy, Default)]
struct Band {
    gr: f32,
}

impl Band {
    #[allow(clippy::too_many_arguments)]
    fn gain(&mut self, peak: f32, threshold: f32, ratio: f32, att: f32, rel: f32) -> f32 {
        let target = Compressor::curve(20.0 * peak.max(1e-9).log10(), threshold, ratio, 6.0);
        let c = if target < self.gr { att } else { rel };
        self.gr = target + (self.gr - target) * c;
        db(self.gr)
    }
}

struct Bands {
    x1: [Crossover; 2],
    x2: [Crossover; 2],
    bands: [Band; 3],
}

impl Bands {
    fn new() -> Self {
        Self { x1: [Crossover::default(); 2], x2: [Crossover::default(); 2], bands: [Band::default(); 3] }
    }

    fn set(&mut self, lo: f32, hi: f32, sr: f32) {
        for c in &mut self.x1 {
            c.set(lo, sr);
        }
        for c in &mut self.x2 {
            c.set(hi.max(lo * 1.5), sr);
        }
    }

    /// Splits one stereo frame into low / mid / high.
    fn split(&mut self, l: f32, r: f32) -> [[f32; 2]; 3] {
        let (ll, lrest) = self.x1[0].split(l);
        let (rl, rrest) = self.x1[1].split(r);
        let (lm, lh) = self.x2[0].split(lrest);
        let (rm, rh) = self.x2[1].split(rrest);
        [[ll, rl], [lm, rm], [lh, rh]]
    }
}

fn coef(ms: f32, sr: f32) -> f32 {
    (-1.0 / (ms.max(0.01) * 0.001 * sr)).exp()
}

pub mod mb {
    pub const LOW_XOVER: u32 = 0;
    pub const HIGH_XOVER: u32 = 1;
    pub const LOW_THRESH: u32 = 2;
    pub const MID_THRESH: u32 = 3;
    pub const HIGH_THRESH: u32 = 4;
    pub const RATIO: u32 = 5;
    pub const ATTACK: u32 = 6;
    pub const RELEASE: u32 = 7;
    pub const LOW_GAIN: u32 = 8;
    pub const MID_GAIN: u32 = 9;
    pub const HIGH_GAIN: u32 = 10;
}

pub static MULTIBAND_PARAMS: [ParamInfo; 11] = [
    cont(mb::LOW_XOVER, "mb.low_xover", "Low / mid", "Crossover", 40.0, 1_000.0, 200.0, "Hz"),
    cont(mb::HIGH_XOVER, "mb.high_xover", "Mid / high", "Crossover", 1_000.0, 12_000.0, 3_000.0, "Hz"),
    cont(mb::LOW_THRESH, "mb.low_thresh", "Low threshold", "Thresholds", -60.0, 0.0, -18.0, "dB"),
    cont(mb::MID_THRESH, "mb.mid_thresh", "Mid threshold", "Thresholds", -60.0, 0.0, -18.0, "dB"),
    cont(mb::HIGH_THRESH, "mb.high_thresh", "High threshold", "Thresholds", -60.0, 0.0, -18.0, "dB"),
    cont(mb::RATIO, "mb.ratio", "Ratio", "Dynamics", 1.0, 20.0, 3.0, ":1"),
    cont(mb::ATTACK, "mb.attack", "Attack", "Dynamics", 0.1, 100.0, 10.0, "ms"),
    cont(mb::RELEASE, "mb.release", "Release", "Dynamics", 10.0, 1_000.0, 150.0, "ms"),
    cont(mb::LOW_GAIN, "mb.low_gain", "Low gain", "Output", -12.0, 12.0, 0.0, "dB"),
    cont(mb::MID_GAIN, "mb.mid_gain", "Mid gain", "Output", -12.0, 12.0, 0.0, "dB"),
    cont(mb::HIGH_GAIN, "mb.high_gain", "High gain", "Output", -12.0, 12.0, 0.0, "dB"),
];

pub struct Multiband {
    sr: f32,
    p: Params<11>,
    b: Bands,
}

impl Multiband {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&MULTIBAND_PARAMS), b: Bands::new() }
    }
}

impl Effect for Multiband {
    crate::effect_common!(MULTIBAND_ID, "Multiband Compressor", MULTIBAND_PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.p.dirty {
            self.b.set(self.p.get(mb::LOW_XOVER), self.p.get(mb::HIGH_XOVER), self.sr);
            self.p.dirty = false;
        }
        let th = [self.p.get(mb::LOW_THRESH), self.p.get(mb::MID_THRESH), self.p.get(mb::HIGH_THRESH)];
        let gains = [db(self.p.get(mb::LOW_GAIN)), db(self.p.get(mb::MID_GAIN)), db(self.p.get(mb::HIGH_GAIN))];
        let ratio = self.p.get(mb::RATIO);
        let (att, rel) = (coef(self.p.get(mb::ATTACK), self.sr), coef(self.p.get(mb::RELEASE), self.sr));
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let split = self.b.split(*l, *r);
            let (mut ol, mut or) = (0.0, 0.0);
            for (i, [bl, br]) in split.into_iter().enumerate() {
                let g = self.b.bands[i].gain(bl.abs().max(br.abs()), th[i], ratio, att, rel) * gains[i];
                ol += bl * g;
                or += br * g;
            }
            *l = ol;
            *r = or;
        }
    }

    fn reset(&mut self) {
        self.b = Bands::new();
        self.p.dirty = true;
    }
}

pub mod po {
    pub const AMOUNT: u32 = 0;
    pub const MIX: u32 = 1;
}

pub static POLISH_PARAMS: [ParamInfo; 2] = [
    cont(po::AMOUNT, "polish.amount", "Amount", "Polish", 0.0, 1.0, 0.5, ""),
    cont(po::MIX, "polish.mix", "Mix", "Polish", 0.0, 1.0, 1.0, ""),
];

pub struct Polish {
    sr: f32,
    p: Params<2>,
    b: Bands,
    air: [Biquad; 2],
    ready: bool,
}

impl Polish {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&POLISH_PARAMS), b: Bands::new(), air: [Biquad::default(); 2], ready: false }
    }
}

impl Effect for Polish {
    crate::effect_common!(POLISH_ID, "Polish", POLISH_PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let amount = self.p.get(po::AMOUNT);
        if self.p.dirty || !self.ready {
            self.b.set(150.0, 4_000.0, self.sr);
            for a in &mut self.air {
                a.set(Shape::HighShelf, 10_000.0, std::f32::consts::FRAC_1_SQRT_2, 4.0 * amount, self.sr);
            }
            self.p.dirty = false;
            self.ready = true;
        }
        let mix = self.p.get(po::MIX);
        // More amount: lower thresholds, more ratio, more makeup, a touch of saturation.
        let th = -6.0 - 24.0 * amount;
        let ratio = 1.0 + 3.0 * amount;
        let makeup = db(6.0 * amount);
        let drive = 1.0 + 1.5 * amount;
        let (att, rel) = (coef(8.0, self.sr), coef(120.0, self.sr));
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let split = self.b.split(*l, *r);
            let (mut ol, mut or) = (0.0, 0.0);
            for (i, [bl, br]) in split.into_iter().enumerate() {
                let g = self.b.bands[i].gain(bl.abs().max(br.abs()), th, ratio, att, rel) * makeup;
                ol += bl * g;
                or += br * g;
            }
            let sat = |x: f32| (x * drive).tanh() / drive.tanh();
            let (yl, yr) = (self.air[0].process(sat(ol)), self.air[1].process(sat(or)));
            *l += (yl - *l) * mix;
            *r += (yr - *r) * mix;
        }
    }

    fn reset(&mut self) {
        self.b = Bands::new();
        self.air.iter_mut().for_each(Biquad::reset);
        self.ready = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testutil::sine_gain;

    #[test]
    fn crossover_bands_sum_flat() {
        // With no compression the three bands must add back to (nearly) the input level.
        let sr = 48_000.0;
        for f in [60.0, 500.0, 2_500.0, 8_000.0] {
            let mut m = Multiband::new(sr);
            for id in [mb::LOW_THRESH, mb::MID_THRESH, mb::HIGH_THRESH] {
                m.set_param(id, 0.0);
            }
            m.set_param(mb::RATIO, 1.0);
            let g = sine_gain(|l, r| m.process(l, r), f, sr);
            assert!((g - 1.0).abs() < 0.05, "{f} Hz: bands sum to x{g}");
        }
    }

    #[test]
    fn band_gain_controls_only_its_band() {
        let sr = 48_000.0;
        let mut m = Multiband::new(sr);
        m.set_param(mb::RATIO, 1.0);
        m.set_param(mb::HIGH_GAIN, -12.0);
        assert!(sine_gain(|l, r| m.process(l, r), 9_000.0, sr) < 0.35, "high band cut");
        let mut m = Multiband::new(sr);
        m.set_param(mb::RATIO, 1.0);
        m.set_param(mb::HIGH_GAIN, -12.0);
        assert!(sine_gain(|l, r| m.process(l, r), 80.0, sr) > 0.9, "lows untouched");
    }

    #[test]
    fn polish_is_dry_at_zero_mix_and_louder_when_on() {
        let sr = 48_000.0;
        let mut p = Polish::new(sr);
        p.set_param(po::MIX, 0.0);
        assert!((sine_gain(|l, r| p.process(l, r), 1000.0, sr) - 1.0).abs() < 0.01);
        let mut p = Polish::new(sr);
        p.set_param(po::AMOUNT, 1.0);
        assert!(sine_gain(|l, r| p.process(l, r), 1000.0, sr) > 1.1, "polish adds level/density");
    }
}
