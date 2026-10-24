//! Phaser (LFO-swept first-order allpass chain) and flanger (LFO-swept short delay).

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont, stepped};

pub const PHASER_ID: &str = "soundesti.phaser";
pub const FLANGER_ID: &str = "soundesti.flanger";

pub mod ph {
    pub const RATE: u32 = 0;
    pub const DEPTH: u32 = 1;
    pub const STAGES: u32 = 2;
    pub const FEEDBACK: u32 = 3;
    pub const MIX: u32 = 4;
}

pub static PHASER_PARAMS: [ParamInfo; 5] = [
    cont(ph::RATE, "phaser.rate", "Rate", "Phaser", 0.02, 8.0, 0.4, "Hz"),
    cont(ph::DEPTH, "phaser.depth", "Depth", "Phaser", 0.0, 1.0, 0.7, ""),
    stepped(ph::STAGES, "phaser.stages", "Stages", "Phaser", 2.0, 1.0, &["4", "6", "8"]),
    cont(ph::FEEDBACK, "phaser.feedback", "Feedback", "Phaser", 0.0, 0.9, 0.4, ""),
    cont(ph::MIX, "phaser.mix", "Mix", "Output", 0.0, 1.0, 0.5, ""),
];

const MAX_STAGES: usize = 8;

pub struct Phaser {
    sr: f32,
    p: Params<5>,
    phase: f32,
    /// Allpass state per channel and stage.
    z: [[f32; MAX_STAGES]; 2],
    fb: [f32; 2],
}

impl Phaser {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&PHASER_PARAMS), phase: 0.0, z: [[0.0; MAX_STAGES]; 2], fb: [0.0; 2] }
    }
}

impl Effect for Phaser {
    crate::effect_common!(PHASER_ID, "Phaser", PHASER_PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let rate = self.p.get(ph::RATE) / self.sr;
        let depth = self.p.get(ph::DEPTH);
        let stages = [4, 6, 8][(self.p.get(ph::STAGES) as usize).min(2)];
        let (fbk, mix) = (self.p.get(ph::FEEDBACK), self.p.get(ph::MIX));
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            for (ch, x) in [l, r].into_iter().enumerate() {
                // Sweep 200 Hz .. 200 Hz * 2^(6*depth) exponentially; right channel 90 degrees later.
                let lfo = 0.5 + 0.5 * ((self.phase + ch as f32 * 0.25) * std::f32::consts::TAU).sin();
                let f = 200.0 * 2f32.powf(6.0 * depth * lfo);
                let t = (std::f32::consts::PI * f / self.sr).tan();
                let a = (t - 1.0) / (t + 1.0);
                let mut y = *x + self.fb[ch] * fbk;
                for z in self.z[ch].iter_mut().take(stages) {
                    let out = a * y + *z;
                    *z = y - a * out;
                    y = out;
                }
                self.fb[ch] = y;
                // Dry and phase-shifted signals sum to create the notches.
                *x = *x * (1.0 - mix * 0.5) + y * mix * 0.5;
            }
            self.phase = (self.phase + rate).fract();
        }
    }

    fn reset(&mut self) {
        self.z = [[0.0; MAX_STAGES]; 2];
        self.fb = [0.0; 2];
    }
}

pub mod fl {
    pub const RATE: u32 = 0;
    pub const DEPTH: u32 = 1;
    pub const DELAY: u32 = 2;
    pub const FEEDBACK: u32 = 3;
    pub const MIX: u32 = 4;
}

pub static FLANGER_PARAMS: [ParamInfo; 5] = [
    cont(fl::RATE, "flanger.rate", "Rate", "Flanger", 0.02, 5.0, 0.25, "Hz"),
    cont(fl::DEPTH, "flanger.depth", "Depth", "Flanger", 0.0, 1.0, 0.6, ""),
    cont(fl::DELAY, "flanger.delay", "Delay", "Flanger", 0.3, 8.0, 2.0, "ms"),
    cont(fl::FEEDBACK, "flanger.feedback", "Feedback", "Flanger", -0.95, 0.95, 0.5, ""),
    cont(fl::MIX, "flanger.mix", "Mix", "Output", 0.0, 1.0, 0.5, ""),
];

pub struct Flanger {
    sr: f32,
    p: Params<5>,
    buf: [Vec<f32>; 2],
    pos: usize,
    phase: f32,
}

impl Flanger {
    pub fn new(sr: f32) -> Self {
        let n = (sr * 0.02) as usize + 4;
        Self { sr, p: Params::new(&FLANGER_PARAMS), buf: [vec![0.0; n], vec![0.0; n]], pos: 0, phase: 0.0 }
    }
}

impl Effect for Flanger {
    crate::effect_common!(FLANGER_ID, "Flanger", FLANGER_PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = self.buf[0].len();
        let rate = self.p.get(fl::RATE) / self.sr;
        let base = self.p.get(fl::DELAY) * 0.001 * self.sr;
        let depth = self.p.get(fl::DEPTH);
        let (fbk, mix) = (self.p.get(fl::FEEDBACK), self.p.get(fl::MIX));
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            for (ch, x) in [l, r].into_iter().enumerate() {
                let lfo = 0.5 + 0.5 * ((self.phase + ch as f32 * 0.25) * std::f32::consts::TAU).sin();
                let d = (base * (1.0 + depth * lfo * 2.0)).clamp(1.0, (n - 3) as f32);
                let rp = self.pos as f32 - d + n as f32;
                let i0 = rp as usize % n;
                let frac = rp.fract();
                let delayed = self.buf[ch][i0] * (1.0 - frac) + self.buf[ch][(i0 + 1) % n] * frac;
                self.buf[ch][self.pos] = *x + delayed * fbk;
                *x = *x * (1.0 - mix * 0.5) + delayed * mix * 0.5;
            }
            self.pos = (self.pos + 1) % n;
            self.phase = (self.phase + rate).fract();
        }
    }

    fn reset(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::testutil::sine_gain;

    #[test]
    fn modulation_effects_change_the_sound_but_stay_dry_at_zero_mix() {
        let sr = 48_000.0;
        for id in [PHASER_ID, FLANGER_ID] {
            let mut fx = crate::create(id, sr).unwrap();
            let mix = crate::params_for(id).iter().find(|p| p.key.ends_with(".mix")).unwrap().id;
            fx.set_param(mix, 0.0);
            assert!((sine_gain(|l, r| fx.process(l, r), 1000.0, sr) - 1.0).abs() < 0.01, "{id}: zero mix is dry");
            let mut fx = crate::create(id, sr).unwrap();
            fx.set_param(mix, 1.0);
            let g = sine_gain(|l, r| fx.process(l, r), 1000.0, sr);
            assert!((g - 1.0).abs() > 0.02 && g.is_finite(), "{id}: no audible effect (x{g})");
        }
    }
}
