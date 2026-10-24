//! The Soundesti project model and `.soundesti` file format.
//!
//! The model is the single source of truth. The audio engine only ever sees
//! snapshots derived from it (see `soundesti-session`).
//!
//! Like FL Studio, step-sequencer steps are not separate data: a lit step *is* a
//! note that starts on that step. The Channel Rack and Piano Roll edit the same notes.

mod file;
mod history;

pub use file::{FILE_EXTENSION, FORMAT_VERSION, ProjectError, load, save};
pub use history::History;

use serde::{Deserialize, Serialize};
use soundesti_timeline::{TICKS_PER_STEP, Tick, TimeSignature};
use std::collections::BTreeMap;

/// The key a new step/note uses by default. FL names MIDI 60 "C5".
pub const DEFAULT_NOTE_KEY: u8 = 60;
/// Default note velocity (FL's 100 of 128).
pub const DEFAULT_VELOCITY: f32 = 100.0 / 128.0;
/// Inserts after the master track.
pub const INSERT_TRACKS: usize = 125;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChannelId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PatternId(pub u32);

pub type Rgb = [u8; 3];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProjectMeta {
    pub title: String,
    pub author: String,
    pub genre: String,
    pub comments: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InstrumentState {
    pub type_id: String,
    /// Parameter values by persistent parameter key.
    #[serde(default)]
    pub params: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub color: Rgb,
    pub instrument: InstrumentState,
    /// Linear gain. FL's default channel volume is 78%.
    pub volume: f32,
    /// -1 (left) .. +1 (right).
    pub pan: f32,
    pub muted: bool,
    /// Mixer track this channel feeds (0 = master).
    pub mixer_track: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub start: Tick,
    pub length: Tick,
    pub key: u8,
    /// 0..1
    pub velocity: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default = "default_release")]
    pub release: f32,
    /// Cents.
    #[serde(default)]
    pub fine_pitch: f32,
    #[serde(default)]
    pub muted: bool,
    /// Note color group 0..15.
    #[serde(default)]
    pub color: u8,
    /// Editor selection; not saved.
    #[serde(skip)]
    pub selected: bool,
}

fn default_release() -> f32 {
    0.5
}

impl Note {
    pub fn new(start: Tick, length: Tick, key: u8) -> Self {
        Self {
            start,
            length: length.max(1),
            key,
            velocity: DEFAULT_VELOCITY,
            pan: 0.0,
            release: default_release(),
            fine_pitch: 0.0,
            muted: false,
            color: 0,
            selected: false,
        }
    }

    pub fn end(&self) -> Tick {
        self.start + self.length
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PatternChannel {
    pub channel: ChannelId,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pattern {
    pub id: PatternId,
    pub name: String,
    #[serde(default)]
    pub color: Option<Rgb>,
    #[serde(default)]
    pub channels: Vec<PatternChannel>,
}

impl Pattern {
    pub fn new(id: PatternId, name: impl Into<String>) -> Self {
        Self { id, name: name.into(), color: None, channels: Vec::new() }
    }

    pub fn notes(&self, ch: ChannelId) -> &[Note] {
        self.channels.iter().find(|c| c.channel == ch).map(|c| c.notes.as_slice()).unwrap_or(&[])
    }

    pub fn notes_mut(&mut self, ch: ChannelId) -> &mut Vec<Note> {
        let idx = match self.channels.iter().position(|c| c.channel == ch) {
            Some(i) => i,
            None => {
                self.channels.push(PatternChannel { channel: ch, notes: Vec::new() });
                self.channels.len() - 1
            }
        };
        &mut self.channels[idx].notes
    }

    pub fn is_empty(&self) -> bool {
        self.channels.iter().all(|c| c.notes.is_empty())
    }

    /// FL-style automatic pattern length: whole bars covering every note, minimum one bar.
    pub fn length_ticks(&self, ts: TimeSignature) -> Tick {
        let bar = ts.ticks_per_bar();
        let end = self.channels.iter().flat_map(|c| c.notes.iter()).map(Note::end).max().unwrap_or(0);
        ((end + bar - 1) / bar).max(1) * bar
    }

    /// True if a note on `ch` starts exactly on step `step`.
    pub fn step_is_on(&self, ch: ChannelId, step: i64) -> bool {
        let t = step * TICKS_PER_STEP;
        self.notes(ch).iter().any(|n| n.start == t)
    }

    /// Step-sequencer click: removes notes starting on the step, or adds a one-step note.
    /// Returns the new state of the step.
    pub fn toggle_step(&mut self, ch: ChannelId, step: i64) -> bool {
        let t = step * TICKS_PER_STEP;
        let notes = self.notes_mut(ch);
        let before = notes.len();
        notes.retain(|n| n.start != t);
        if notes.len() != before {
            return false;
        }
        notes.push(Note::new(t, TICKS_PER_STEP, DEFAULT_NOTE_KEY));
        notes.sort_by_key(|n| (n.start, n.key));
        true
    }

    pub fn remove_channel(&mut self, ch: ChannelId) {
        self.channels.retain(|c| c.channel != ch);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MixerTrack {
    pub name: String,
    #[serde(default)]
    pub color: Option<Rgb>,
    pub volume: f32,
    pub pan: f32,
    pub muted: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub format_version: u32,
    #[serde(default)]
    pub meta: ProjectMeta,
    pub bpm: f64,
    #[serde(default)]
    pub time_signature: TimeSignature,
    pub channels: Vec<Channel>,
    pub patterns: Vec<Pattern>,
    pub current_pattern: PatternId,
    pub mixer: Vec<MixerTrack>,
    #[serde(default)]
    pub metronome: bool,
    next_channel_id: u32,
    next_pattern_id: u32,
}

/// Channel colors cycle through this original palette.
pub const CHANNEL_COLORS: [Rgb; 8] = [
    [94, 180, 255],
    [255, 145, 77],
    [130, 220, 120],
    [235, 100, 150],
    [180, 140, 255],
    [255, 210, 80],
    [80, 210, 200],
    [220, 220, 220],
];

impl Default for Project {
    fn default() -> Self {
        Self::new()
    }
}

impl Project {
    /// An empty project: one pattern, master + 125 inserts, no channels.
    pub fn empty() -> Self {
        let mut mixer = vec![MixerTrack { name: "Master".into(), color: None, volume: 0.8, pan: 0.0, muted: false }];
        mixer.extend((1..=INSERT_TRACKS).map(|i| MixerTrack {
            name: format!("Insert {i}"),
            color: None,
            volume: 0.8,
            pan: 0.0,
            muted: false,
        }));
        Self {
            format_version: FORMAT_VERSION,
            meta: ProjectMeta::default(),
            bpm: 130.0,
            time_signature: TimeSignature::default(),
            channels: Vec::new(),
            patterns: vec![Pattern::new(PatternId(1), "Pattern 1")],
            current_pattern: PatternId(1),
            mixer,
            metronome: false,
            next_channel_id: 1,
            next_pattern_id: 2,
        }
    }

    /// The default new-project template: one Trident channel.
    pub fn new() -> Self {
        let mut p = Self::empty();
        p.add_channel("Trident", "soundesti.trident");
        p
    }

    pub fn add_channel(&mut self, name: impl Into<String>, instrument_type: &str) -> ChannelId {
        let id = ChannelId(self.next_channel_id);
        self.next_channel_id += 1;
        let color = CHANNEL_COLORS[self.channels.len() % CHANNEL_COLORS.len()];
        self.channels.push(Channel {
            id,
            name: name.into(),
            color,
            instrument: InstrumentState { type_id: instrument_type.into(), params: BTreeMap::new() },
            volume: 0.78,
            pan: 0.0,
            muted: false,
            mixer_track: 0,
        });
        id
    }

    pub fn remove_channel(&mut self, id: ChannelId) -> bool {
        let before = self.channels.len();
        self.channels.retain(|c| c.id != id);
        for p in &mut self.patterns {
            p.remove_channel(id);
        }
        self.channels.len() != before
    }

    /// Clones a channel (settings and its notes in every pattern), inserted after the original.
    pub fn clone_channel(&mut self, id: ChannelId) -> Option<ChannelId> {
        let idx = self.channels.iter().position(|c| c.id == id)?;
        let new_id = ChannelId(self.next_channel_id);
        self.next_channel_id += 1;
        let mut ch = self.channels[idx].clone();
        ch.id = new_id;
        ch.name = format!("{} #2", ch.name);
        self.channels.insert(idx + 1, ch);
        for p in &mut self.patterns {
            let notes = p.notes(id).to_vec();
            if !notes.is_empty() {
                *p.notes_mut(new_id) = notes;
            }
        }
        Some(new_id)
    }

    pub fn channel(&self, id: ChannelId) -> Option<&Channel> {
        self.channels.iter().find(|c| c.id == id)
    }

    pub fn channel_mut(&mut self, id: ChannelId) -> Option<&mut Channel> {
        self.channels.iter_mut().find(|c| c.id == id)
    }

    pub fn pattern(&self, id: PatternId) -> Option<&Pattern> {
        self.patterns.iter().find(|p| p.id == id)
    }

    pub fn pattern_mut(&mut self, id: PatternId) -> Option<&mut Pattern> {
        self.patterns.iter_mut().find(|p| p.id == id)
    }

    pub fn current(&self) -> &Pattern {
        self.pattern(self.current_pattern).unwrap_or(&self.patterns[0])
    }

    pub fn current_mut(&mut self) -> &mut Pattern {
        let id = self.current_pattern;
        let idx = self.patterns.iter().position(|p| p.id == id).unwrap_or(0);
        &mut self.patterns[idx]
    }

    pub fn add_pattern(&mut self) -> PatternId {
        let id = PatternId(self.next_pattern_id);
        self.next_pattern_id += 1;
        self.patterns.push(Pattern::new(id, format!("Pattern {}", id.0)));
        id
    }

    /// FL's F4: the first empty pattern after the current one, creating it if needed.
    pub fn next_empty_pattern(&mut self) -> PatternId {
        let cur = self.patterns.iter().position(|p| p.id == self.current_pattern).unwrap_or(0);
        if let Some(p) = self.patterns.iter().skip(cur + 1).find(|p| p.is_empty()) {
            return p.id;
        }
        self.add_pattern()
    }

    /// Repairs references after loading or heavy edits (never panics on bad files).
    pub fn validate(&mut self) {
        if self.patterns.is_empty() {
            self.patterns.push(Pattern::new(PatternId(self.next_pattern_id.max(1)), "Pattern 1"));
        }
        if self.pattern(self.current_pattern).is_none() {
            self.current_pattern = self.patterns[0].id;
        }
        while self.mixer.len() < INSERT_TRACKS + 1 {
            let i = self.mixer.len();
            self.mixer.push(MixerTrack {
                name: format!("Insert {i}"),
                color: None,
                volume: 0.8,
                pan: 0.0,
                muted: false,
            });
        }
        let max_track = self.mixer.len() as u16 - 1;
        let max_ch = self.channels.iter().map(|c| c.id.0).max().unwrap_or(0);
        let max_pat = self.patterns.iter().map(|p| p.id.0).max().unwrap_or(0);
        self.next_channel_id = self.next_channel_id.max(max_ch + 1);
        self.next_pattern_id = self.next_pattern_id.max(max_pat + 1);
        for c in &mut self.channels {
            c.mixer_track = c.mixer_track.min(max_track);
            c.volume = finite_or(c.volume, 0.78).clamp(0.0, 2.0);
            c.pan = finite_or(c.pan, 0.0).clamp(-1.0, 1.0);
        }
        for t in &mut self.mixer {
            t.volume = finite_or(t.volume, 0.8).clamp(0.0, 2.0);
            t.pan = finite_or(t.pan, 0.0).clamp(-1.0, 1.0);
        }
        self.bpm = finite_or(self.bpm as f32, 130.0).clamp(10.0, 522.0) as f64;
        let ids: Vec<ChannelId> = self.channels.iter().map(|c| c.id).collect();
        for p in &mut self.patterns {
            p.channels.retain(|pc| ids.contains(&pc.channel));
            for pc in &mut p.channels {
                pc.notes.retain(|n| n.start >= 0 && n.length > 0 && n.key < 128);
                for n in &mut pc.notes {
                    n.velocity = finite_or(n.velocity, DEFAULT_VELOCITY).clamp(0.0, 1.0);
                }
            }
        }
    }
}

fn finite_or(v: f32, default: f32) -> f32 {
    if v.is_finite() { v } else { default }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_project_shape() {
        let p = Project::new();
        assert_eq!(p.mixer.len(), 126);
        assert_eq!(p.mixer[0].name, "Master");
        assert_eq!(p.channels.len(), 1);
        assert_eq!(p.current().name, "Pattern 1");
    }

    #[test]
    fn steps_are_notes() {
        let mut p = Project::new();
        let ch = p.channels[0].id;
        assert!(p.current_mut().toggle_step(ch, 4));
        assert!(p.current().step_is_on(ch, 4));
        let n = p.current().notes(ch)[0];
        assert_eq!((n.start, n.length, n.key), (4 * TICKS_PER_STEP, TICKS_PER_STEP, DEFAULT_NOTE_KEY));
        assert!(!p.current_mut().toggle_step(ch, 4));
        assert!(p.current().notes(ch).is_empty());
    }

    #[test]
    fn pattern_length_rounds_up_to_bars() {
        let ts = TimeSignature::default();
        let mut pat = Pattern::new(PatternId(1), "p");
        assert_eq!(pat.length_ticks(ts), ts.ticks_per_bar());
        pat.notes_mut(ChannelId(1)).push(Note::new(ts.ticks_per_bar(), 1, 60));
        assert_eq!(pat.length_ticks(ts), 2 * ts.ticks_per_bar());
    }

    #[test]
    fn removing_channel_removes_its_notes() {
        let mut p = Project::new();
        let a = p.channels[0].id;
        let b = p.add_channel("B", "soundesti.trident");
        p.current_mut().toggle_step(a, 0);
        p.current_mut().toggle_step(b, 0);
        assert!(p.remove_channel(a));
        assert_eq!(p.current().channels.len(), 1);
        assert_eq!(p.current().channels[0].channel, b);
    }

    #[test]
    fn clone_channel_copies_notes() {
        let mut p = Project::new();
        let a = p.channels[0].id;
        p.current_mut().toggle_step(a, 2);
        let b = p.clone_channel(a).unwrap();
        assert_eq!(p.channels[1].id, b);
        assert!(p.current().step_is_on(b, 2));
    }

    #[test]
    fn next_empty_pattern_reuses_or_creates() {
        let mut p = Project::new();
        let first = p.next_empty_pattern();
        assert_eq!(first, PatternId(2));
        assert_eq!(p.patterns.len(), 2);
        // Pattern 2 is empty, so asking again from pattern 1 returns it instead of creating a third.
        assert_eq!(p.next_empty_pattern(), PatternId(2));
        assert_eq!(p.patterns.len(), 2);
    }

    #[test]
    fn validate_repairs_bad_data() {
        let mut p = Project::new();
        p.current_pattern = PatternId(999);
        p.channels[0].mixer_track = 9999;
        p.channels[0].volume = f32::NAN;
        p.mixer.truncate(3);
        p.bpm = f64::INFINITY;
        p.validate();
        assert_eq!(p.current_pattern, PatternId(1));
        assert_eq!(p.mixer.len(), 126);
        assert_eq!(p.channels[0].mixer_track, 125);
        assert_eq!(p.channels[0].volume, 0.78);
        assert_eq!(p.bpm, 130.0);
    }
}
