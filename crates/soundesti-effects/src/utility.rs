//! Utility: gain, pan and mid/side stereo width.

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont, db};

pub const TYPE_ID: &str = "soundesti.utility";

pub mod p {
    pub const GAIN: u32 = 0;
    pub const PAN: u32 = 1;
    pub const WIDTH: u32 = 2;
}

pub static PARAMS: [ParamInfo; 3] = [
    cont(p::GAIN, "util.gain", "Gain", "Utility", -24.0, 24.0, 0.0, "dB"),
    cont(p::PAN, "util.pan", "Pan", "Utility", -1.0, 1.0, 0.0, ""),
    cont(p::WIDTH, "util.width", "Width", "Utility", 0.0, 2.0, 1.0, ""),
];

pub struct Utility {
    p: Params<3>,
}

impl Utility {
    pub fn new(_sr: f32) -> Self {
        Self { p: Params::new(&PARAMS) }
    }
}

impl Effect for Utility {
    crate::effect_common!(TYPE_ID, "Utility", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let g = db(self.p.get(p::GAIN));
        let (pl, pr) = soundesti_dsp::balance_pan(self.p.get(p::PAN));
        let w = self.p.get(p::WIDTH);
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let (m, s) = ((*l + *r) * 0.5, (*l - *r) * 0.5 * w);
            *l = (m + s) * g * pl;
            *r = (m - s) * g * pr;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_is_identity_and_width_zero_is_mono() {
        let mut u = Utility::new(48_000.0);
        let (mut l, mut r) = (vec![0.5, -0.2], vec![0.1, 0.3]);
        u.process(&mut l, &mut r);
        assert!((l[0] - 0.5).abs() < 1e-6 && (r[1] - 0.3).abs() < 1e-6);
        u.set_param(p::WIDTH, 0.0);
        let (mut l, mut r) = (vec![1.0], vec![0.0]);
        u.process(&mut l, &mut r);
        assert!((l[0] - r[0]).abs() < 1e-6);
    }
}
