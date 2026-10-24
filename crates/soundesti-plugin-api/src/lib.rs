//! Native Soundesti plugin interface.
//!
//! Instruments run on the audio thread. Implementations must not allocate, lock,
//! or do I/O inside [`Instrument::process`], [`Instrument::set_param`] or the
//! note handlers. All buffers are preallocated in the constructor.

/// Identifies a note so overlapping notes on the same key stay independent.
pub type NoteId = u32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum NoteEvent {
    On {
        id: NoteId,
        key: u8,
        velocity: f32,
    },
    Off {
        id: NoteId,
    },
    /// Release every sounding voice normally.
    AllNotesOff,
    /// Cut everything immediately (Ctrl+H panic).
    AllSoundOff,
}

/// A note event positioned `offset` frames into the current block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimedEvent {
    pub offset: u32,
    pub event: NoteEvent,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParamKind {
    Continuous,
    /// Integer steps between `min` and `max`; `labels` names each step if non-empty.
    Stepped {
        labels: &'static [&'static str],
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamInfo {
    /// Persistent ID. Never renumber: projects and automation refer to it.
    pub id: u32,
    /// Persistent string key used in project files.
    pub key: &'static str,
    pub name: &'static str,
    pub group: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: &'static str,
    pub kind: ParamKind,
}

pub trait Instrument: Send {
    /// Stable type identifier stored in projects, e.g. `"soundesti.trident"`.
    fn type_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn params(&self) -> &'static [ParamInfo];
    fn get_param(&self, id: u32) -> f32;
    fn set_param(&mut self, id: u32, value: f32);

    /// Renders one block, *adding* into `left`/`right`. `events` are sorted by offset
    /// and every offset is `< left.len()`.
    fn process(&mut self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]);

    /// Number of voices currently sounding (for diagnostics and tests).
    fn active_voices(&self) -> usize;

    /// Gives a sample-based instrument its audio. Returns the previous sample so the caller
    /// can free it off the audio thread. Instruments that don't use samples hand it back.
    fn set_sample(&mut self, sample: Option<std::sync::Arc<SampleData>>) -> Option<std::sync::Arc<SampleData>> {
        sample
    }
}

/// An insert effect. Runs on the audio thread: no allocation, locks or I/O in `process`,
/// `set_param` or `set_tempo`.
pub trait Effect: Send {
    fn type_id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn params(&self) -> &'static [ParamInfo];
    fn get_param(&self, id: u32) -> f32;
    fn set_param(&mut self, id: u32, value: f32);
    /// Processes one stereo block in place.
    fn process(&mut self, left: &mut [f32], right: &mut [f32]);
    /// Project tempo, for tempo-synced effects.
    fn set_tempo(&mut self, _bpm: f32) {}
    /// Clears internal state (delay lines, envelopes), e.g. on transport stop.
    fn reset(&mut self) {}
}

/// Decoded audio for sample-based instruments (stereo, any sample rate).
#[derive(Clone, Debug, PartialEq)]
pub struct SampleData {
    pub left: Vec<f32>,
    pub right: Vec<f32>,
    pub sample_rate: f32,
    pub name: String,
}

impl SampleData {
    pub fn len(&self) -> usize {
        self.left.len().min(self.right.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn duration_seconds(&self) -> f32 {
        self.len() as f32 / self.sample_rate.max(1.0)
    }
}

/// Helper for instruments: splits a block at each event offset and calls `render`
/// for every sub-range, calling `handle` for each event at its sample position.
pub fn for_each_segment<S>(
    state: &mut S,
    events: &[TimedEvent],
    frames: usize,
    mut handle: impl FnMut(&mut S, NoteEvent),
    mut render: impl FnMut(&mut S, std::ops::Range<usize>),
) {
    let mut pos = 0usize;
    for ev in events {
        let at = (ev.offset as usize).min(frames);
        if at > pos {
            render(state, pos..at);
            pos = at;
        }
        handle(state, ev.event);
    }
    if pos < frames {
        render(state, pos..frames);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_cover_block_in_order() {
        let events = [
            TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key: 60, velocity: 1.0 } },
            TimedEvent { offset: 10, event: NoteEvent::Off { id: 1 } },
            TimedEvent { offset: 10, event: NoteEvent::AllNotesOff },
        ];
        let mut log: Vec<String> = Vec::new();
        for_each_segment(
            &mut log,
            &events,
            32,
            |l, e| l.push(format!("{e:?}")),
            |l, r| l.push(format!("{}..{}", r.start, r.end)),
        );
        assert_eq!(log.len(), 5);
        assert!(log[0].starts_with("On"));
        assert_eq!(log[1], "0..10");
        assert!(log[2].starts_with("Off"));
        assert_eq!(log[4], "10..32");
    }
}
