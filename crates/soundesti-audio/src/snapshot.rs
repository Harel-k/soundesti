use soundesti_timeline::{TempoMap, Tick, TimeSignature};

/// One note as the engine plays it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaybackNote {
    /// Engine channel slot.
    pub channel: u16,
    pub start: Tick,
    pub end: Tick,
    pub key: u8,
    pub velocity: f32,
}

/// Immutable description of what to play, built on the control thread.
/// Replacing it is a single pointer swap on the audio thread.
#[derive(Clone, Debug)]
pub struct PlaybackSnapshot {
    pub tempo: TempoMap,
    pub time_signature: TimeSignature,
    pub loop_start: Tick,
    pub loop_end: Tick,
    /// Sorted by start tick. Notes are clipped to the loop range.
    pub notes: Vec<PlaybackNote>,
}

impl PlaybackSnapshot {
    pub fn new(
        tempo: TempoMap,
        time_signature: TimeSignature,
        loop_start: Tick,
        loop_end: Tick,
        mut notes: Vec<PlaybackNote>,
    ) -> Self {
        let loop_end = loop_end.max(loop_start + 1);
        notes.retain(|n| n.start >= loop_start && n.start < loop_end && n.end > n.start);
        for n in &mut notes {
            n.end = n.end.min(loop_end);
        }
        notes.sort_by_key(|n| (n.start, n.channel, n.key));
        Self { tempo, time_signature, loop_start, loop_end, notes }
    }

    pub fn empty(bpm: f64) -> Self {
        let ts = TimeSignature::default();
        Self::new(TempoMap::constant(bpm), ts, 0, ts.ticks_per_bar(), Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_sorted_and_clipped() {
        let n = |start, end| PlaybackNote { channel: 0, start, end, key: 60, velocity: 1.0 };
        let s = PlaybackSnapshot::new(
            TempoMap::constant(120.0),
            TimeSignature::default(),
            0,
            3840,
            vec![n(960, 1200), n(0, 5000), n(4000, 4100), n(10, 10)],
        );
        assert_eq!(s.notes.len(), 2);
        assert_eq!(s.notes[0].start, 0);
        assert_eq!(s.notes[0].end, 3840);
        assert_eq!(s.notes[1].start, 960);
    }
}
