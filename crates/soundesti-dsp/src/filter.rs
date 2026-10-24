/// Filter response of the state-variable filter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SvfMode {
    LowPass,
    HighPass,
    BandPass,
}

/// Topology-preserving-transform state-variable filter (trapezoidal integration).
/// Stable under fast cutoff modulation, unlike direct-form biquads.
#[derive(Clone, Debug)]
pub struct Svf {
    ic1: f32,
    ic2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
    k: f32,
}

impl Default for Svf {
    fn default() -> Self {
        let mut f = Self { ic1: 0.0, ic2: 0.0, a1: 0.0, a2: 0.0, a3: 0.0, k: 0.0 };
        f.set(1000.0, 0.707, 48_000.0);
        f
    }
}

impl Svf {
    /// `q` >= 0.5; ~0.707 is Butterworth.
    pub fn set(&mut self, cutoff: f32, q: f32, sample_rate: f32) {
        let fc = cutoff.clamp(10.0, sample_rate * 0.49);
        let g = (std::f32::consts::PI * fc / sample_rate).tan();
        self.k = 1.0 / q.max(0.5);
        self.a1 = 1.0 / (1.0 + g * (g + self.k));
        self.a2 = g * self.a1;
        self.a3 = g * self.a2;
    }

    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }

    #[inline]
    pub fn process(&mut self, x: f32, mode: SvfMode) -> f32 {
        let v3 = x - self.ic2;
        let v1 = self.a1 * self.ic1 + self.a2 * v3;
        let v2 = self.ic2 + self.a2 * self.ic1 + self.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        match mode {
            SvfMode::LowPass => v2,
            SvfMode::BandPass => v1,
            SvfMode::HighPass => x - self.k * v1 - v2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Steady-state RMS gain of the filter for a sine at `freq`.
    fn gain_at(mode: SvfMode, cutoff: f32, freq: f32) -> f32 {
        let sr = 48_000.0;
        let mut f = Svf::default();
        f.set(cutoff, 0.707, sr);
        let n = 48_000;
        let mut in_sq = 0.0f64;
        let mut out_sq = 0.0f64;
        for i in 0..n {
            let x = (std::f32::consts::TAU * freq * i as f32 / sr).sin();
            let y = f.process(x, mode);
            if i > n / 2 {
                in_sq += (x * x) as f64;
                out_sq += (y * y) as f64;
            }
        }
        (out_sq / in_sq).sqrt() as f32
    }

    #[test]
    fn lowpass_passes_low_and_cuts_high() {
        assert!((gain_at(SvfMode::LowPass, 1000.0, 100.0) - 1.0).abs() < 0.02);
        // 12 dB/oct: two octaves up ~ -24 dB.
        assert!(gain_at(SvfMode::LowPass, 1000.0, 4000.0) < 0.1);
    }

    #[test]
    fn cutoff_is_minus_3db_for_butterworth() {
        let g = gain_at(SvfMode::LowPass, 1000.0, 1000.0);
        assert!((g - 0.707).abs() < 0.03, "{g}");
    }

    #[test]
    fn highpass_mirrors_lowpass() {
        assert!(gain_at(SvfMode::HighPass, 1000.0, 100.0) < 0.05);
        assert!((gain_at(SvfMode::HighPass, 1000.0, 10_000.0) - 1.0).abs() < 0.05);
    }

    #[test]
    fn stays_finite_with_extreme_settings() {
        let mut f = Svf::default();
        for i in 0..10_000 {
            f.set(if i % 2 == 0 { 10.0 } else { 23_000.0 }, 40.0, 48_000.0);
            let y = f.process(if i % 100 < 50 { 1.0 } else { -1.0 }, SvfMode::LowPass);
            assert!(y.is_finite());
        }
    }
}
