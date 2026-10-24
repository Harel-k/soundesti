//! Small helpers shared by the native instruments.

use soundesti_plugin_api::{ParamInfo, ParamKind};

/// Builds a continuous parameter description.
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

/// Builds a stepped parameter description.
#[allow(clippy::too_many_arguments)]
pub const fn stepped(
    id: u32,
    key: &'static str,
    name: &'static str,
    group: &'static str,
    min: f32,
    max: f32,
    default: f32,
    labels: &'static [&'static str],
) -> ParamInfo {
    ParamInfo { id, key, name, group, min, max, default, unit: "", kind: ParamKind::Stepped { labels } }
}

/// Parameter storage indexed like a `PARAMS` table; clamps and rejects non-finite values.
pub struct ParamStore<const N: usize> {
    pub table: &'static [ParamInfo; N],
    pub values: [f32; N],
}

impl<const N: usize> ParamStore<N> {
    pub fn new(table: &'static [ParamInfo; N]) -> Self {
        let mut values = [0.0; N];
        for (v, p) in values.iter_mut().zip(table.iter()) {
            *v = p.default;
        }
        Self { table, values }
    }

    pub fn index(&self, id: u32) -> Option<usize> {
        self.table.iter().position(|p| p.id == id)
    }

    pub fn get(&self, id: u32) -> f32 {
        self.index(id).map(|i| self.values[i]).unwrap_or(0.0)
    }

    /// Stores a value, returning the clamped value actually stored.
    pub fn set(&mut self, id: u32, value: f32) -> Option<f32> {
        let i = self.index(id)?;
        let p = self.table[i];
        let mut v = if value.is_finite() { value.clamp(p.min, p.max) } else { p.default };
        if matches!(p.kind, ParamKind::Stepped { .. }) {
            v = v.round();
        }
        self.values[i] = v;
        Some(v)
    }
}

/// Picks a voice without allocating: the first idle one, otherwise the oldest.
pub fn pick_voice<V>(voices: &[V], idle: impl Fn(&V) -> bool, age: impl Fn(&V) -> u64) -> usize {
    if let Some(i) = voices.iter().position(&idle) {
        return i;
    }
    voices.iter().enumerate().min_by_key(|(_, v)| age(v)).map(|(i, _)| i).unwrap_or(0)
}

/// Semitone ratio.
#[inline]
pub fn semis(st: f32) -> f32 {
    2f32.powf(st / 12.0)
}

/// Exponential decay coefficient so a value falls to -60 dB in `seconds`.
#[inline]
pub fn decay_coef(seconds: f32, sample_rate: f32) -> f32 {
    (-6.9078 / (seconds.max(0.001) * sample_rate)).exp()
}
