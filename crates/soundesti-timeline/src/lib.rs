//! Musical time for Soundesti.
//!
//! All musical positions are integer ticks at [`PPQ`] ticks per quarter note.
//! Conversions to seconds/samples are stateless functions of a [`TempoMap`], so
//! the engine never accumulates floating-point drift: it keeps an integer sample
//! counter and asks the tempo map where that sample lands musically.

use serde::{Deserialize, Serialize};

/// Ticks per quarter note. 960 divides evenly by 2, 3, 4, 5, 6, 8, 10, 12, 16, 32,
/// so triplets, quintuplets and 1/32 grids are all exact.
pub const PPQ: i64 = 960;

/// A musical position or duration in ticks.
pub type Tick = i64;

/// One step of the FL-style step sequencer: a 1/16 note.
pub const TICKS_PER_STEP: Tick = PPQ / 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSignature {
    pub numerator: u8,
    pub denominator: u8,
}

impl Default for TimeSignature {
    fn default() -> Self {
        Self::new(4, 4)
    }
}

impl TimeSignature {
    pub const fn new(numerator: u8, denominator: u8) -> Self {
        Self { numerator, denominator }
    }

    /// Ticks in one beat (one denominator note).
    pub fn ticks_per_beat(&self) -> Tick {
        PPQ * 4 / self.denominator.max(1) as Tick
    }

    pub fn ticks_per_bar(&self) -> Tick {
        self.ticks_per_beat() * self.numerator.max(1) as Tick
    }

    pub fn steps_per_bar(&self) -> i64 {
        self.ticks_per_bar() / TICKS_PER_STEP
    }
}

/// Position split into 1-based bar and beat plus remaining ticks, like a DAW clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarBeatTick {
    pub bar: i64,
    pub beat: i64,
    pub tick: i64,
}

impl BarBeatTick {
    pub fn from_tick(tick: Tick, ts: TimeSignature) -> Self {
        let bar_len = ts.ticks_per_bar();
        let beat_len = ts.ticks_per_beat();
        let bar = tick.div_euclid(bar_len);
        let in_bar = tick.rem_euclid(bar_len);
        Self { bar: bar + 1, beat: in_bar / beat_len + 1, tick: in_bar % beat_len }
    }

    pub fn to_tick(self, ts: TimeSignature) -> Tick {
        (self.bar - 1) * ts.ticks_per_bar() + (self.beat - 1) * ts.ticks_per_beat() + self.tick
    }
}

impl std::fmt::Display for BarBeatTick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{:02}:{:03}", self.bar, self.beat, self.tick)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct TempoPoint {
    pub tick: Tick,
    pub bpm: f64,
}

/// Piecewise-constant tempo map. The first point is always at tick 0.
#[derive(Clone, Debug, PartialEq)]
pub struct TempoMap {
    points: Vec<TempoPoint>,
    /// Seconds elapsed at each point's tick (same index as `points`).
    seconds_at: Vec<f64>,
}

pub const MIN_BPM: f64 = 10.0;
pub const MAX_BPM: f64 = 522.0;

impl TempoMap {
    pub fn constant(bpm: f64) -> Self {
        Self::from_points(vec![TempoPoint { tick: 0, bpm }])
    }

    /// Builds a map from arbitrary points: sorts them, clamps BPM, and inserts a
    /// point at tick 0 if missing (using the earliest point's tempo).
    pub fn from_points(mut points: Vec<TempoPoint>) -> Self {
        if points.is_empty() {
            points.push(TempoPoint { tick: 0, bpm: 120.0 });
        }
        points.sort_by_key(|p| p.tick);
        points.dedup_by_key(|p| p.tick);
        if points[0].tick != 0 {
            let bpm = points[0].bpm;
            points.insert(0, TempoPoint { tick: 0, bpm });
        }
        for p in &mut points {
            p.bpm = if p.bpm.is_finite() { p.bpm.clamp(MIN_BPM, MAX_BPM) } else { 120.0 };
        }
        let mut seconds_at = Vec::with_capacity(points.len());
        let mut acc = 0.0;
        for (i, p) in points.iter().enumerate() {
            if i > 0 {
                let prev = points[i - 1];
                acc += ticks_to_seconds_at(p.tick - prev.tick, prev.bpm);
            }
            seconds_at.push(acc);
        }
        Self { points, seconds_at }
    }

    pub fn points(&self) -> &[TempoPoint] {
        &self.points
    }

    pub fn bpm_at(&self, tick: Tick) -> f64 {
        self.points[self.segment_for_tick(tick as f64)].bpm
    }

    fn segment_for_tick(&self, tick: f64) -> usize {
        self.points.partition_point(|p| (p.tick as f64) <= tick).saturating_sub(1)
    }

    fn segment_for_seconds(&self, seconds: f64) -> usize {
        self.seconds_at.partition_point(|&s| s <= seconds).saturating_sub(1)
    }

    /// Seconds from tick 0 to `tick` (fractional ticks allowed).
    pub fn seconds_at_tick(&self, tick: f64) -> f64 {
        let i = self.segment_for_tick(tick);
        let p = self.points[i];
        self.seconds_at[i] + (tick - p.tick as f64) * 60.0 / (p.bpm * PPQ as f64)
    }

    /// Fractional tick reached after `seconds`.
    pub fn tick_at_seconds(&self, seconds: f64) -> f64 {
        let i = self.segment_for_seconds(seconds);
        let p = self.points[i];
        p.tick as f64 + (seconds - self.seconds_at[i]) * p.bpm * PPQ as f64 / 60.0
    }

    /// Sample index (rounded to nearest) at which `tick` occurs.
    pub fn sample_at_tick(&self, tick: f64, sample_rate: f64) -> i64 {
        (self.seconds_at_tick(tick) * sample_rate).round() as i64
    }

    pub fn tick_at_sample(&self, sample: i64, sample_rate: f64) -> f64 {
        self.tick_at_seconds(sample as f64 / sample_rate)
    }
}

fn ticks_to_seconds_at(ticks: Tick, bpm: f64) -> f64 {
    ticks as f64 * 60.0 / (bpm * PPQ as f64)
}

/// Snap `tick` to the nearest multiple of `grid` (grid <= 0 means no snap).
pub fn snap_round(tick: Tick, grid: Tick) -> Tick {
    if grid <= 0 {
        return tick;
    }
    ((tick as f64 / grid as f64).round() as Tick) * grid
}

/// Snap `tick` down to the previous multiple of `grid` (grid <= 0 means no snap).
pub fn snap_floor(tick: Tick, grid: Tick) -> Tick {
    if grid <= 0 {
        return tick;
    }
    tick.div_euclid(grid) * grid
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn one_beat_at_120_bpm_is_half_a_second() {
        let map = TempoMap::constant(120.0);
        assert!((map.seconds_at_tick(PPQ as f64) - 0.5).abs() < 1e-12);
        assert_eq!(map.sample_at_tick(PPQ as f64, 48_000.0), 24_000);
    }

    #[test]
    fn tempo_change_mid_song() {
        // 1 bar at 120 (2 s), then 60 BPM: next beat takes 1 s.
        let ts = TimeSignature::default();
        let bar = ts.ticks_per_bar();
        let map = TempoMap::from_points(vec![TempoPoint { tick: 0, bpm: 120.0 }, TempoPoint { tick: bar, bpm: 60.0 }]);
        assert!((map.seconds_at_tick(bar as f64) - 2.0).abs() < 1e-12);
        assert!((map.seconds_at_tick((bar + PPQ) as f64) - 3.0).abs() < 1e-12);
        assert!((map.tick_at_seconds(3.0) - (bar + PPQ) as f64).abs() < 1e-9);
        assert_eq!(map.bpm_at(bar - 1), 120.0);
        assert_eq!(map.bpm_at(bar), 60.0);
    }

    #[test]
    fn missing_zero_point_is_inserted_and_bpm_clamped() {
        let map = TempoMap::from_points(vec![TempoPoint { tick: 960, bpm: 9999.0 }]);
        assert_eq!(map.points()[0].tick, 0);
        assert_eq!(map.points()[0].bpm, MAX_BPM);
    }

    #[test]
    fn bbt_display_and_round_trip() {
        let ts = TimeSignature::default();
        let t = BarBeatTick::from_tick(0, ts);
        assert_eq!(t.to_string(), "1:01:000");
        let t = BarBeatTick::from_tick(ts.ticks_per_bar() + PPQ * 2 + 17, ts);
        assert_eq!((t.bar, t.beat, t.tick), (2, 3, 17));
        assert_eq!(t.to_tick(ts), ts.ticks_per_bar() + PPQ * 2 + 17);
    }

    #[test]
    fn odd_time_signatures() {
        let ts = TimeSignature::new(7, 8);
        assert_eq!(ts.ticks_per_beat(), PPQ / 2);
        assert_eq!(ts.ticks_per_bar(), PPQ / 2 * 7);
        assert_eq!(ts.steps_per_bar(), 14);
    }

    #[test]
    fn snapping() {
        assert_eq!(snap_round(130, 240), 240);
        assert_eq!(snap_round(110, 240), 0);
        assert_eq!(snap_floor(479, 240), 240);
        assert_eq!(snap_floor(-1, 240), -240);
        assert_eq!(snap_round(77, 0), 77);
    }

    #[test]
    fn long_song_has_no_drift() {
        // 10,000 bars at an awkward tempo: converting the sample position back must land
        // within one sample's worth of ticks of the exact value.
        let map = TempoMap::constant(137.3);
        let sr = 44_100.0;
        let tick = 10_000.0 * TimeSignature::default().ticks_per_bar() as f64;
        let s = map.sample_at_tick(tick, sr);
        let back = map.tick_at_sample(s, sr);
        let ticks_per_sample = 137.3 * PPQ as f64 / 60.0 / sr;
        assert!((back - tick).abs() <= ticks_per_sample);
    }

    proptest! {
        #[test]
        fn seconds_tick_round_trip(
            bpms in proptest::collection::vec(20.0f64..400.0, 1..8),
            tick in 0i64..2_000_000,
        ) {
            let points = bpms.iter().enumerate()
                .map(|(i, &bpm)| TempoPoint { tick: i as i64 * 3840, bpm })
                .collect();
            let map = TempoMap::from_points(points);
            let s = map.seconds_at_tick(tick as f64);
            prop_assert!((map.tick_at_seconds(s) - tick as f64).abs() < 1e-6);
        }

        #[test]
        fn seconds_monotonic(a in 0i64..1_000_000, b in 0i64..1_000_000) {
            let map = TempoMap::from_points(vec![
                TempoPoint { tick: 0, bpm: 90.0 },
                TempoPoint { tick: 50_000, bpm: 170.0 },
            ]);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            prop_assert!(map.seconds_at_tick(lo as f64) <= map.seconds_at_tick(hi as f64));
        }
    }
}
