/// ADSR times in seconds, sustain as a 0..1 level.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AdsrParams {
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
}

impl Default for AdsrParams {
    fn default() -> Self {
        Self { attack: 0.002, decay: 0.3, sustain: 0.7, release: 0.15 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Linear-segment ADSR. Release always starts from the current level, so a
/// note-off during attack does not jump.
#[derive(Clone, Debug)]
pub struct Adsr {
    stage: Stage,
    level: f32,
    rate: f32,
    params: AdsrParams,
    sample_rate: f32,
}

/// Minimum segment length; keeps a 0 ms attack/release click-free.
const MIN_SECONDS: f32 = 0.0005;

impl Adsr {
    pub fn new(sample_rate: f32) -> Self {
        Self { stage: Stage::Idle, level: 0.0, rate: 0.0, params: AdsrParams::default(), sample_rate }
    }

    pub fn set_params(&mut self, params: AdsrParams) {
        self.params = params;
    }

    fn samples(&self, seconds: f32) -> f32 {
        (seconds.max(MIN_SECONDS) * self.sample_rate).max(1.0)
    }

    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
        self.rate = (1.0 - self.level).max(0.0) / self.samples(self.params.attack);
    }

    pub fn gate_off(&mut self) {
        if self.stage == Stage::Idle {
            return;
        }
        self.stage = Stage::Release;
        self.rate = self.level / self.samples(self.params.release);
    }

    /// Fast fade used for voice stealing (~2 ms).
    pub fn kill(&mut self) {
        if self.stage == Stage::Idle {
            return;
        }
        self.stage = Stage::Release;
        self.rate = self.level / self.samples(0.002);
    }

    pub fn is_idle(&self) -> bool {
        self.stage == Stage::Idle
    }

    pub fn is_releasing(&self) -> bool {
        self.stage == Stage::Release
    }

    pub fn level(&self) -> f32 {
        self.level
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += self.rate;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                    let sustain = self.params.sustain.clamp(0.0, 1.0);
                    self.rate = (1.0 - sustain) / self.samples(self.params.decay);
                }
            }
            Stage::Decay => {
                let sustain = self.params.sustain.clamp(0.0, 1.0);
                self.level -= self.rate;
                if self.level <= sustain {
                    self.level = sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => {
                // Follow live sustain changes smoothly enough for a test synth.
                self.level = self.params.sustain.clamp(0.0, 1.0);
                if self.level <= 0.0 {
                    self.stage = Stage::Idle;
                }
            }
            Stage::Release => {
                self.level -= self.rate;
                if self.level <= 0.0 {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_cycle() {
        let sr = 1000.0;
        let mut e = Adsr::new(sr);
        e.set_params(AdsrParams { attack: 0.01, decay: 0.01, sustain: 0.5, release: 0.01 });
        e.gate_on();
        let attack: Vec<f32> = (0..10).map(|_| e.next()).collect();
        assert!((attack[9] - 1.0).abs() < 1e-5, "{attack:?}");
        for _ in 0..20 {
            e.next();
        }
        assert!((e.level() - 0.5).abs() < 1e-5);
        e.gate_off();
        for _ in 0..10 {
            e.next();
        }
        assert!(e.is_idle());
        assert_eq!(e.level(), 0.0);
    }

    #[test]
    fn release_during_attack_does_not_jump() {
        let mut e = Adsr::new(48_000.0);
        e.set_params(AdsrParams { attack: 0.1, decay: 0.1, sustain: 1.0, release: 0.1 });
        e.gate_on();
        for _ in 0..100 {
            e.next();
        }
        let before = e.level();
        e.gate_off();
        let after = e.next();
        assert!(after <= before && before - after < 0.001);
    }

    #[test]
    fn zero_times_are_still_click_free() {
        let mut e = Adsr::new(48_000.0);
        e.set_params(AdsrParams { attack: 0.0, decay: 0.0, sustain: 1.0, release: 0.0 });
        e.gate_on();
        let first = e.next();
        assert!(first < 0.1, "attack jumped to {first}");
    }

    #[test]
    fn gate_off_when_idle_is_noop() {
        let mut e = Adsr::new(48_000.0);
        e.gate_off();
        assert!(e.is_idle());
    }
}
