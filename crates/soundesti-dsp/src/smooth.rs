/// Linear-ramp parameter smoother: reaches the target in a fixed time, which keeps
/// gain changes click-free without the long tail of a one-pole filter.
#[derive(Clone, Debug)]
pub struct Smoothed {
    current: f32,
    target: f32,
    step: f32,
    remaining: u32,
    ramp_samples: u32,
}

impl Smoothed {
    pub fn new(value: f32, sample_rate: f32, ramp_ms: f32) -> Self {
        Self {
            current: value,
            target: value,
            step: 0.0,
            remaining: 0,
            ramp_samples: ((sample_rate * ramp_ms / 1000.0) as u32).max(1),
        }
    }

    pub fn set_target(&mut self, target: f32) {
        if target == self.target {
            return;
        }
        self.target = target;
        self.remaining = self.ramp_samples;
        self.step = (target - self.current) / self.ramp_samples as f32;
    }

    /// Jumps straight to `value` (use when transport is stopped or on load).
    pub fn reset(&mut self, value: f32) {
        self.current = value;
        self.target = value;
        self.remaining = 0;
    }

    #[inline]
    pub fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.current = if self.remaining == 0 { self.target } else { self.current + self.step };
        }
        self.current
    }

    pub fn is_smoothing(&self) -> bool {
        self.remaining > 0
    }

    pub fn target(&self) -> f32 {
        self.target
    }

    pub fn current(&self) -> f32 {
        self.current
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reaches_target_exactly_after_ramp() {
        let mut s = Smoothed::new(0.0, 48_000.0, 10.0);
        s.set_target(1.0);
        let n = 480;
        let mut last = 0.0;
        for i in 0..n {
            let v = s.next();
            assert!(v >= last, "monotonic at {i}");
            last = v;
        }
        assert_eq!(s.current(), 1.0);
        assert!(!s.is_smoothing());
    }

    #[test]
    fn no_step_larger_than_ramp_increment() {
        let mut s = Smoothed::new(1.0, 48_000.0, 5.0);
        s.set_target(0.0);
        let mut prev = 1.0;
        for _ in 0..240 {
            let v = s.next();
            assert!((prev - v) <= 1.0 / 240.0 + 1e-6);
            prev = v;
        }
    }
}
