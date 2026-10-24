//! Shared helpers: parameter storage and RBJ-cookbook biquads.

use soundesti_plugin_api::{ParamInfo, ParamKind};

#[allow(clippy::too_many_arguments)]
pub const fn cont(
    id: u32,
    key: &'static str,
    name: &'static str,
    group: &'static str,
    min: f32,
    max: f32,
    default: f32,
    unit: &'static str,
) -> ParamInfo {
    ParamInfo { id, key, name, group, min, max, default, unit, kind: ParamKind::Continuous }
}

#[allow(clippy::too_many_arguments)]
pub const fn stepped(
    id: u32,
    key: &'static str,
    name: &'static str,
    group: &'static str,
    max: f32,
    default: f32,
    labels: &'static [&'static str],
) -> ParamInfo {
    ParamInfo { id, key, name, group, min: 0.0, max, default, unit: "", kind: ParamKind::Stepped { labels } }
}

pub struct Params<const N: usize> {
    table: &'static [ParamInfo; N],
    pub v: [f32; N],
    pub dirty: bool,
}

impl<const N: usize> Params<N> {
    pub fn new(table: &'static [ParamInfo; N]) -> Self {
        let mut v = [0.0; N];
        for (x, p) in v.iter_mut().zip(table) {
            *x = p.default;
        }
        Self { table, v, dirty: true }
    }

    pub fn get(&self, id: u32) -> f32 {
        self.table.iter().position(|p| p.id == id).map(|i| self.v[i]).unwrap_or(0.0)
    }

    pub fn set(&mut self, id: u32, value: f32) {
        if let Some(i) = self.table.iter().position(|p| p.id == id) {
            let p = self.table[i];
            let mut x = if value.is_finite() { value.clamp(p.min, p.max) } else { p.default };
            if matches!(p.kind, ParamKind::Stepped { .. }) {
                x = x.round();
            }
            self.v[i] = x;
            self.dirty = true;
        }
    }
}

pub fn db(x: f32) -> f32 {
    10f32.powf(x / 20.0)
}

/// Transposed direct form II biquad.
#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Default for Biquad {
    fn default() -> Self {
        Self { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    LowShelf,
    HighShelf,
    Peak,
    HighPass,
    LowPass,
}

impl Biquad {
    /// RBJ Audio EQ Cookbook coefficients.
    pub fn set(&mut self, shape: Shape, freq: f32, q: f32, gain_db: f32, sr: f32) {
        let f = freq.clamp(10.0, sr * 0.49);
        let w = std::f32::consts::TAU * f / sr;
        let (sw, cw) = w.sin_cos();
        let q = q.max(0.1);
        let alpha = sw / (2.0 * q);
        let a = 10f32.powf(gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match shape {
            Shape::Peak => (1.0 + alpha * a, -2.0 * cw, 1.0 - alpha * a, 1.0 + alpha / a, -2.0 * cw, 1.0 - alpha / a),
            Shape::LowShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cw + s),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cw),
                    a * ((a + 1.0) - (a - 1.0) * cw - s),
                    (a + 1.0) + (a - 1.0) * cw + s,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cw),
                    (a + 1.0) + (a - 1.0) * cw - s,
                )
            }
            Shape::HighShelf => {
                let s = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cw + s),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cw),
                    a * ((a + 1.0) + (a - 1.0) * cw - s),
                    (a + 1.0) - (a - 1.0) * cw + s,
                    2.0 * ((a - 1.0) - (a + 1.0) * cw),
                    (a + 1.0) - (a - 1.0) * cw - s,
                )
            }
            Shape::HighPass => ((1.0 + cw) / 2.0, -(1.0 + cw), (1.0 + cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
            Shape::LowPass => ((1.0 - cw) / 2.0, 1.0 - cw, (1.0 - cw) / 2.0, 1.0 + alpha, -2.0 * cw, 1.0 - alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = a1 / a0;
        self.a2 = a2 / a0;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }
}

/// Implements the boilerplate parts of `Effect` for a struct with a `p: Params<N>` field.
#[macro_export]
macro_rules! effect_common {
    ($id:expr, $name:expr, $params:expr) => {
        fn type_id(&self) -> &'static str {
            $id
        }
        fn display_name(&self) -> &'static str {
            $name
        }
        fn params(&self) -> &'static [soundesti_plugin_api::ParamInfo] {
            &$params
        }
        fn get_param(&self, id: u32) -> f32 {
            self.p.get(id)
        }
        fn set_param(&mut self, id: u32, value: f32) {
            self.p.set(id, value);
        }
    };
}

#[cfg(test)]
pub mod testutil {
    /// Steady-state gain of a mono process function for a sine at `freq`.
    pub fn sine_gain(mut f: impl FnMut(&mut [f32], &mut [f32]), freq: f32, sr: f32) -> f32 {
        let n = sr as usize;
        let mut l: Vec<f32> = (0..n).map(|i| (std::f32::consts::TAU * freq * i as f32 / sr).sin() * 0.25).collect();
        let mut r = l.clone();
        let input = l.clone();
        f(&mut l, &mut r);
        let rms = |v: &[f32]| (v[n / 2..].iter().map(|x| x * x).sum::<f32>() / (n / 2) as f32).sqrt();
        rms(&l) / rms(&input)
    }
}
