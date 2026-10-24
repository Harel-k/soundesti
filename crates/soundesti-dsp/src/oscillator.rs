use crate::Noise;

/// Oscillator waveform. Saw and square are band-limited with PolyBLEP.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Triangle,
    Saw,
    Square,
    Noise,
}

impl Waveform {
    pub const ALL: [Waveform; 5] =
        [Waveform::Sine, Waveform::Triangle, Waveform::Saw, Waveform::Square, Waveform::Noise];

    pub fn name(self) -> &'static str {
        match self {
            Waveform::Sine => "Sine",
            Waveform::Triangle => "Triangle",
            Waveform::Saw => "Saw",
            Waveform::Square => "Square",
            Waveform::Noise => "Noise",
        }
    }

    /// Maps a continuous parameter value (0..=4) to a waveform.
    pub fn from_param(v: f32) -> Self {
        Self::ALL[(v.round().clamp(0.0, 4.0)) as usize]
    }

    pub fn to_param(self) -> f32 {
        Self::ALL.iter().position(|&x| x == self).unwrap_or(0) as f32
    }
}

/// Phase-accumulator oscillator.
#[derive(Clone, Debug)]
pub struct Oscillator {
    phase: f32,
    inc: f32,
    noise: Noise,
}

impl Oscillator {
    pub fn new(seed: u32) -> Self {
        Self { phase: 0.0, inc: 0.0, noise: Noise::new(seed) }
    }

    pub fn set_phase(&mut self, phase: f32) {
        self.phase = phase.rem_euclid(1.0);
    }

    pub fn set_frequency(&mut self, freq: f32, sample_rate: f32) {
        // Clamp below Nyquist so PolyBLEP stays well-defined.
        self.inc = (freq / sample_rate).clamp(0.0, 0.49);
    }

    #[inline]
    pub fn next(&mut self, wave: Waveform) -> f32 {
        let t = self.phase;
        let dt = self.inc;
        let out = match wave {
            Waveform::Sine => (t * std::f32::consts::TAU).sin(),
            Waveform::Triangle => 1.0 - 4.0 * (t - 0.5).abs(),
            Waveform::Saw => (2.0 * t - 1.0) - poly_blep(t, dt),
            Waveform::Square => {
                let naive = if t < 0.5 { 1.0 } else { -1.0 };
                naive + poly_blep(t, dt) - poly_blep((t + 0.5).rem_euclid(1.0), dt)
            }
            Waveform::Noise => self.noise.next(),
        };
        self.phase += dt;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        out
    }
}

/// Polynomial band-limited step residual.
#[inline]
fn poly_blep(t: f32, dt: f32) -> f32 {
    if dt <= 0.0 {
        0.0
    } else if t < dt {
        let x = t / dt;
        x + x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + x + x + 1.0
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(wave: Waveform, freq: f32, sr: f32, n: usize) -> Vec<f32> {
        let mut o = Oscillator::new(7);
        o.set_frequency(freq, sr);
        (0..n).map(|_| o.next(wave)).collect()
    }

    fn rising_zero_crossings(v: &[f32]) -> usize {
        v.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count()
    }

    #[test]
    fn sine_frequency_is_correct() {
        // 1 second of 440 Hz must contain 440 (+-1) rising zero crossings.
        let v = render(Waveform::Sine, 440.0, 48_000.0, 48_000);
        let z = rising_zero_crossings(&v) as i64;
        assert!((z - 440).abs() <= 1, "crossings {z}");
    }

    #[test]
    fn all_waves_bounded_and_finite() {
        for w in Waveform::ALL {
            for freq in [20.0, 440.0, 5_000.0, 20_000.0] {
                let v = render(w, freq, 44_100.0, 4096);
                assert!(v.iter().all(|x| x.is_finite() && x.abs() <= 1.6), "{w:?} {freq}");
            }
        }
    }

    #[test]
    fn saw_has_no_dc() {
        let v = render(Waveform::Saw, 100.0, 48_000.0, 48_000);
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean.abs() < 0.01, "dc {mean}");
    }

    #[test]
    fn param_mapping_round_trips() {
        for w in Waveform::ALL {
            assert_eq!(Waveform::from_param(w.to_param()), w);
        }
    }
}
