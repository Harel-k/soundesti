//! Real-time-safe DSP building blocks. Nothing in this crate allocates after construction.

// Per-sample generators use `next()` by DSP convention; they are not iterators.
#![allow(clippy::should_implement_trait)]

pub mod envelope;
pub mod filter;
pub mod oscillator;
pub mod smooth;

pub use envelope::{Adsr, AdsrParams};
pub use filter::{Svf, SvfMode};
pub use oscillator::{Oscillator, Waveform};
pub use smooth::Smoothed;

/// Converts decibels to linear gain.
#[inline]
pub fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Converts linear gain to decibels (`-inf` for 0).
#[inline]
pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.abs().log10()
}

/// Frequency in Hz of a (fractional) MIDI key, A4 = 69 = 440 Hz.
#[inline]
pub fn midi_to_freq(key: f32) -> f32 {
    440.0 * 2f32.powf((key - 69.0) / 12.0)
}

/// Balance-style pan: centre keeps both channels at unity, panning attenuates the
/// opposite side linearly. `pan` is -1 (left) .. +1 (right).
#[inline]
pub fn balance_pan(pan: f32) -> (f32, f32) {
    let pan = pan.clamp(-1.0, 1.0);
    ((1.0 - pan).min(1.0), (1.0 + pan).min(1.0))
}

/// Replaces NaN/inf with silence. Used as a last line of defence on output buses.
#[inline]
pub fn sanitize(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// Enables flush-to-zero / denormals-are-zero on the calling thread (x86/x86_64).
/// Call once at the start of every audio callback thread.
pub fn enable_flush_denormals() {
    #[cfg(target_arch = "x86_64")]
    #[allow(deprecated)]
    unsafe {
        use std::arch::x86_64::{_mm_getcsr, _mm_setcsr};
        // FTZ = bit 15, DAZ = bit 6.
        _mm_setcsr(_mm_getcsr() | 0x8040);
    }
}

/// Small, fast, deterministic PRNG for noise generators (xorshift32).
#[derive(Clone, Debug)]
pub struct Noise {
    state: u32,
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self { state: seed.max(1) }
    }

    /// Uniform white noise in -1..1.
    #[inline]
    pub fn next(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        (x as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_round_trip() {
        for db in [-60.0f32, -6.0, 0.0, 6.0] {
            assert!((gain_to_db(db_to_gain(db)) - db).abs() < 1e-4);
        }
        assert!((db_to_gain(-6.0206) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn a4_is_440() {
        assert!((midi_to_freq(69.0) - 440.0).abs() < 1e-3);
        assert!((midi_to_freq(81.0) - 880.0).abs() < 1e-2);
    }

    #[test]
    fn pan_law_centre_is_unity() {
        assert_eq!(balance_pan(0.0), (1.0, 1.0));
        assert_eq!(balance_pan(-1.0), (1.0, 0.0));
        assert_eq!(balance_pan(1.0), (0.0, 1.0));
        assert_eq!(balance_pan(0.5), (0.5, 1.0));
    }

    #[test]
    fn noise_is_bounded_and_not_constant() {
        let mut n = Noise::new(1);
        let v: Vec<f32> = (0..10_000).map(|_| n.next()).collect();
        assert!(v.iter().all(|x| (-1.0..=1.0).contains(x)));
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        assert!(mean.abs() < 0.05, "mean {mean}");
    }
}
