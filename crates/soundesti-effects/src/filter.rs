//! Resonant state-variable filter (low/high/band pass) for sweeps and automation.

use soundesti_dsp::{Svf, SvfMode};
use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont, stepped};

pub const TYPE_ID: &str = "soundesti.filter";

pub mod p {
    pub const MODE: u32 = 0;
    pub const CUTOFF: u32 = 1;
    pub const RESONANCE: u32 = 2;
    pub const MIX: u32 = 3;
}

pub static PARAMS: [ParamInfo; 4] = [
    stepped(p::MODE, "filter.mode", "Mode", "Filter", 2.0, 0.0, &["Low pass", "High pass", "Band pass"]),
    cont(p::CUTOFF, "filter.cutoff", "Cutoff", "Filter", 20.0, 20_000.0, 2_000.0, "Hz"),
    cont(p::RESONANCE, "filter.resonance", "Resonance", "Filter", 0.5, 12.0, 1.0, "Q"),
    cont(p::MIX, "filter.mix", "Mix", "Filter", 0.0, 1.0, 1.0, ""),
];

pub struct Filter {
    sr: f32,
    p: Params<4>,
    f: [Svf; 2],
}

impl Filter {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&PARAMS), f: [Svf::default(), Svf::default()] }
    }
}

impl Effect for Filter {
    crate::effect_common!(TYPE_ID, "Filter", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mode = match self.p.get(p::MODE) as u32 {
            1 => SvfMode::HighPass,
            2 => SvfMode::BandPass,
            _ => SvfMode::LowPass,
        };
        let (c, q, mix) = (self.p.get(p::CUTOFF), self.p.get(p::RESONANCE), self.p.get(p::MIX));
        for (ch, buf) in [left, right].into_iter().enumerate() {
            self.f[ch].set(c, q, self.sr);
            for x in buf.iter_mut() {
                let y = self.f[ch].process(*x, mode);
                *x += (y - *x) * mix;
            }
        }
    }

    fn reset(&mut self) {
        self.f.iter_mut().for_each(Svf::reset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testutil::sine_gain;

    #[test]
    fn lowpass_cuts_highs() {
        let mut f = Filter::new(48_000.0);
        f.set_param(p::CUTOFF, 500.0);
        assert!(sine_gain(|l, r| f.process(l, r), 8_000.0, 48_000.0) < 0.02);
    }
}
