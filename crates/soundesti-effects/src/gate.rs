//! Noise gate with hold and a range (how far closed the gate goes).

use soundesti_plugin_api::{Effect, ParamInfo};

use crate::base::{Params, cont, db};

pub const TYPE_ID: &str = "soundesti.gate";

pub mod p {
    pub const THRESHOLD: u32 = 0;
    pub const ATTACK: u32 = 1;
    pub const HOLD: u32 = 2;
    pub const RELEASE: u32 = 3;
    pub const RANGE: u32 = 4;
}

pub static PARAMS: [ParamInfo; 5] = [
    cont(p::THRESHOLD, "gate.threshold", "Threshold", "Gate", -80.0, 0.0, -40.0, "dB"),
    cont(p::ATTACK, "gate.attack", "Attack", "Timing", 0.1, 50.0, 1.0, "ms"),
    cont(p::HOLD, "gate.hold", "Hold", "Timing", 0.0, 500.0, 50.0, "ms"),
    cont(p::RELEASE, "gate.release", "Release", "Timing", 5.0, 2000.0, 150.0, "ms"),
    cont(p::RANGE, "gate.range", "Range", "Gate", -80.0, 0.0, -60.0, "dB"),
];

pub struct Gate {
    sr: f32,
    p: Params<5>,
    gain: f32,
    hold_left: usize,
}

impl Gate {
    pub fn new(sr: f32) -> Self {
        Self { sr, p: Params::new(&PARAMS), gain: 0.0, hold_left: 0 }
    }
}

impl Effect for Gate {
    crate::effect_common!(TYPE_ID, "Gate", PARAMS);

    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let open_at = db(self.p.get(p::THRESHOLD));
        let floor = db(self.p.get(p::RANGE));
        let coef = |ms: f32| (-1.0 / (ms.max(0.01) * 0.001 * self.sr)).exp();
        let (att, rel) = (coef(self.p.get(p::ATTACK)), coef(self.p.get(p::RELEASE)));
        let hold = (self.p.get(p::HOLD) * 0.001 * self.sr) as usize;
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let level = l.abs().max(r.abs());
            let target = if level >= open_at {
                self.hold_left = hold;
                1.0
            } else if self.hold_left > 0 {
                self.hold_left -= 1;
                1.0
            } else {
                floor
            };
            let c = if target > self.gain { att } else { rel };
            self.gain = target + (self.gain - target) * c;
            *l *= self.gain;
            *r *= self.gain;
        }
    }

    fn reset(&mut self) {
        self.gain = 0.0;
        self.hold_left = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_passes_loud_and_mutes_quiet() {
        let sr = 48_000.0;
        let run = |amp: f32| {
            let mut g = Gate::new(sr);
            let mut l: Vec<f32> = (0..24_000).map(|i| (i as f32 * 0.06).sin() * amp).collect();
            let mut r = l.clone();
            g.process(&mut l, &mut r);
            l[12_000..].iter().fold(0.0f32, |m, x| m.max(x.abs())) / amp
        };
        assert!(run(0.5) > 0.95, "loud signal passes");
        assert!(run(0.001) < 0.01, "quiet signal is gated (-60 dB signal under a -40 dB threshold)");
    }
}
