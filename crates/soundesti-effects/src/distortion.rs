//! Distortion: soft clip, hard clip, wavefold, bitcrush; post tone filter; dry/wet.

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont, db, stepped};

pub const TYPE_ID: &str = "soundesti.distortion";

pub mod p {
    pub const MODE: u32 = 0;
    pub const DRIVE: u32 = 1;
    pub const TONE: u32 = 2;
    pub const MIX: u32 = 3;
    pub const OUTPUT: u32 = 4;
}

pub static PARAMS: [ParamInfo; 5] = [
    stepped(p::MODE, "dist.mode", "Mode", "Drive", 3.0, 0.0, &["Soft clip", "Hard clip", "Wavefold", "Bitcrush"]),
    cont(p::DRIVE, "dist.drive", "Drive", "Drive", 0.0, 48.0, 12.0, "dB"),
    cont(p::TONE, "dist.tone", "Tone", "Tone", 500.0, 20_000.0, 12_000.0, "Hz"),
    cont(p::MIX, "dist.mix", "Mix", "Output", 0.0, 1.0, 1.0, ""),
    cont(p::OUTPUT, "dist.output", "Output", "Output", -24.0, 6.0, -6.0, "dB"),
];

pub struct Distortion {
    sr: f32,
    p: Params<5>,
    lp: [f32; 2],
}

impl Distortion {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&PARAMS), lp: [0.0; 2] }
    }
}

fn shape(x: f32, mode: u32, drive_db: f32) -> f32 {
    match mode {
        1 => x.clamp(-1.0, 1.0),
        2 => {
            // Fold back into [-1, 1] (triangle-wave folding).
            let t = (x + 1.0).rem_euclid(4.0);
            if t < 2.0 { t - 1.0 } else { 3.0 - t }
        }
        3 => {
            // More drive = fewer bits (16 down to 2).
            let bits = (16.0 - drive_db / 48.0 * 14.0).max(2.0);
            let q = 2f32.powf(bits - 1.0);
            (x.clamp(-1.0, 1.0) * q).round() / q
        }
        _ => x.tanh(),
    }
}

impl Effect for Distortion {
    crate::effect_common!(TYPE_ID, "Distortion", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let mode = self.p.get(p::MODE) as u32;
        let drive_db = self.p.get(p::DRIVE);
        // Bitcrush uses drive for bit depth, so don't also boost the level there.
        let drive = if mode == 3 { 1.0 } else { db(drive_db) };
        let a = (std::f32::consts::TAU * self.p.get(p::TONE) / self.sr).min(1.0);
        let (mix, out) = (self.p.get(p::MIX), db(self.p.get(p::OUTPUT)));
        for (ch, buf) in [left, right].into_iter().enumerate() {
            for x in buf.iter_mut() {
                let y = shape(*x * drive, mode, drive_db);
                self.lp[ch] += a * (y - self.lp[ch]);
                *x += (self.lp[ch] * out - *x) * mix;
            }
        }
    }

    fn reset(&mut self) {
        self.lp = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_modes_stay_bounded() {
        for mode in 0..4 {
            for x in [-100.0f32, -1.5, -0.3, 0.0, 0.7, 3.0, 100.0] {
                let y = shape(x, mode, 24.0);
                assert!(y.is_finite() && y.abs() <= 1.0001, "mode {mode} x {x} -> {y}");
            }
        }
    }
}
