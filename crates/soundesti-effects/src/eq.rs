//! 5-band EQ: low cut, low shelf, mid peak, high shelf, high cut.

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Biquad, Params, Shape, cont};

pub const TYPE_ID: &str = "soundesti.eq";

pub mod p {
    pub const LOW_CUT: u32 = 0;
    pub const LOW_FREQ: u32 = 1;
    pub const LOW_GAIN: u32 = 2;
    pub const MID_FREQ: u32 = 3;
    pub const MID_GAIN: u32 = 4;
    pub const MID_Q: u32 = 5;
    pub const HIGH_FREQ: u32 = 6;
    pub const HIGH_GAIN: u32 = 7;
    pub const HIGH_CUT: u32 = 8;
    pub const OUTPUT: u32 = 9;
}

pub static PARAMS: [ParamInfo; 10] = [
    cont(p::LOW_CUT, "lowcut.freq", "Low cut", "Cuts", 20.0, 1000.0, 20.0, "Hz"),
    cont(p::LOW_FREQ, "low.freq", "Low freq", "Low", 30.0, 1000.0, 120.0, "Hz"),
    cont(p::LOW_GAIN, "low.gain", "Low gain", "Low", -18.0, 18.0, 0.0, "dB"),
    cont(p::MID_FREQ, "mid.freq", "Mid freq", "Mid", 150.0, 10_000.0, 1_000.0, "Hz"),
    cont(p::MID_GAIN, "mid.gain", "Mid gain", "Mid", -18.0, 18.0, 0.0, "dB"),
    cont(p::MID_Q, "mid.q", "Mid width (Q)", "Mid", 0.3, 8.0, 1.0, "Q"),
    cont(p::HIGH_FREQ, "high.freq", "High freq", "High", 1_500.0, 16_000.0, 8_000.0, "Hz"),
    cont(p::HIGH_GAIN, "high.gain", "High gain", "High", -18.0, 18.0, 0.0, "dB"),
    cont(p::HIGH_CUT, "highcut.freq", "High cut", "Cuts", 1_000.0, 20_000.0, 20_000.0, "Hz"),
    cont(p::OUTPUT, "out.gain", "Output", "Output", -18.0, 18.0, 0.0, "dB"),
];

pub struct Eq {
    sr: f32,
    p: Params<10>,
    bands: [[Biquad; 5]; 2],
    active: [bool; 5],
    out: f32,
}

impl Eq {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&PARAMS), bands: [[Biquad::default(); 5]; 2], active: [false; 5], out: 1.0 }
    }

    fn update(&mut self) {
        let g = |id| self.p.get(id);
        let specs = [
            (Shape::HighPass, g(p::LOW_CUT), 0.707, 0.0, g(p::LOW_CUT) > 20.5),
            (Shape::LowShelf, g(p::LOW_FREQ), 0.707, g(p::LOW_GAIN), g(p::LOW_GAIN).abs() > 0.01),
            (Shape::Peak, g(p::MID_FREQ), g(p::MID_Q), g(p::MID_GAIN), g(p::MID_GAIN).abs() > 0.01),
            (Shape::HighShelf, g(p::HIGH_FREQ), 0.707, g(p::HIGH_GAIN), g(p::HIGH_GAIN).abs() > 0.01),
            (Shape::LowPass, g(p::HIGH_CUT), 0.707, 0.0, g(p::HIGH_CUT) < 19_999.0),
        ];
        for (i, (shape, f, q, gain, on)) in specs.into_iter().enumerate() {
            self.active[i] = on;
            for ch in 0..2 {
                self.bands[ch][i].set(shape, f, q, gain, self.sr);
            }
        }
        self.out = crate::base::db(g(p::OUTPUT));
        self.p.dirty = false;
    }
}

impl Effect for Eq {
    crate::effect_common!(TYPE_ID, "EQ", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        if self.p.dirty {
            self.update();
        }
        for (ch, buf) in [left, right].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let mut y = *x;
                for (b, on) in self.bands[ch].iter_mut().zip(self.active) {
                    if on {
                        y = b.process(y);
                    }
                }
                *x = y * self.out;
            }
        }
    }

    fn reset(&mut self) {
        self.bands.iter_mut().flatten().for_each(Biquad::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testutil::sine_gain;

    #[test]
    fn flat_eq_is_transparent_and_bands_work() {
        let sr = 48_000.0;
        let mut eq = Eq::new(sr);
        assert!((sine_gain(|l, r| eq.process(l, r), 1000.0, sr) - 1.0).abs() < 0.01, "flat EQ changes level");
        let mut eq = Eq::new(sr);
        eq.set_param(p::LOW_CUT, 200.0);
        assert!(sine_gain(|l, r| eq.process(l, r), 40.0, sr) < 0.1, "low cut must remove 40 Hz");
        let mut eq = Eq::new(sr);
        eq.set_param(p::MID_GAIN, 12.0);
        let g = sine_gain(|l, r| eq.process(l, r), 1000.0, sr);
        assert!((g - 3.98).abs() < 0.3, "+12 dB at the mid frequency, got x{g}");
    }
}
