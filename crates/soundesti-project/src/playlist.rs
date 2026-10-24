//! The Playlist: pattern clips arranged on tracks (FL's song arrangement).
//!
//! A clip shows its pattern from `offset` for `length` ticks starting at `start`. Notes
//! outside the clip window are not played (slip editing moves `offset`).

use serde::{Deserialize, Serialize};
use soundesti_timeline::{Tick, TimeSignature};

use crate::{PatternId, Rgb};

pub const DEFAULT_TRACKS: usize = 50;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistTrack {
    pub name: String,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub color: Option<Rgb>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub pattern: PatternId,
    pub track: u16,
    pub start: Tick,
    pub length: Tick,
    /// How far into the pattern the clip starts (slip edit / slice).
    #[serde(default)]
    pub offset: Tick,
    #[serde(default)]
    pub muted: bool,
    #[serde(skip)]
    pub selected: bool,
}

impl Clip {
    pub fn end(&self) -> Tick {
        self.start + self.length
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub tracks: Vec<PlaylistTrack>,
    #[serde(default)]
    pub clips: Vec<Clip>,
}

impl Default for Playlist {
    fn default() -> Self {
        Self {
            tracks: (1..=DEFAULT_TRACKS)
                .map(|i| PlaylistTrack { name: format!("Track {i}"), muted: false, color: None })
                .collect(),
            clips: Vec::new(),
        }
    }
}

impl Playlist {
    /// Song length: whole bars covering every clip, at least one bar.
    pub fn song_length(&self, ts: TimeSignature) -> Tick {
        let bar = ts.ticks_per_bar();
        let end = self.clips.iter().map(Clip::end).max().unwrap_or(0);
        ((end + bar - 1) / bar).max(1) * bar
    }

    /// Clips that sound (clip and its track unmuted).
    pub fn audible_clips(&self) -> impl Iterator<Item = &Clip> {
        self.clips.iter().filter(|c| !c.muted && !self.tracks.get(c.track as usize).is_some_and(|t| t.muted))
    }

    pub fn clip_at(&self, track: u16, tick: Tick) -> Option<usize> {
        self.clips.iter().rposition(|c| c.track == track && tick >= c.start && tick < c.end())
    }

    /// Splits clip `i` at `tick` (FL slice). Returns false if `tick` is not inside it.
    pub fn slice(&mut self, i: usize, tick: Tick) -> bool {
        let Some(c) = self.clips.get(i).copied() else { return false };
        if tick <= c.start || tick >= c.end() {
            return false;
        }
        let left_len = tick - c.start;
        self.clips[i].length = left_len;
        self.clips.push(Clip { start: tick, length: c.length - left_len, offset: c.offset + left_len, ..c });
        true
    }

    /// Makes sure the track list covers `track`.
    pub fn ensure_track(&mut self, track: usize) {
        while self.tracks.len() <= track {
            let n = self.tracks.len() + 1;
            self.tracks.push(PlaylistTrack { name: format!("Track {n}"), muted: false, color: None });
        }
    }

    pub fn remove_pattern(&mut self, p: PatternId) {
        self.clips.retain(|c| c.pattern != p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(track: u16, start: Tick, length: Tick) -> Clip {
        Clip { pattern: PatternId(1), track, start, length, offset: 0, muted: false, selected: false }
    }

    #[test]
    fn song_length_and_lookup() {
        let ts = TimeSignature::default();
        let mut p = Playlist::default();
        assert_eq!(p.tracks.len(), DEFAULT_TRACKS);
        assert_eq!(p.song_length(ts), ts.ticks_per_bar());
        p.clips.push(clip(0, 0, 3840));
        p.clips.push(clip(2, 3840 * 2, 100));
        assert_eq!(p.song_length(ts), 3840 * 3);
        assert_eq!(p.clip_at(0, 100), Some(0));
        assert_eq!(p.clip_at(1, 100), None);
        assert_eq!(p.clip_at(2, 3840 * 2 + 50), Some(1));
    }

    #[test]
    fn slicing_keeps_content_continuous() {
        let mut p = Playlist::default();
        p.clips.push(clip(0, 960, 3840));
        assert!(!p.slice(0, 960), "slice at the edge does nothing");
        assert!(p.slice(0, 960 + 1000));
        let (a, b) = (p.clips[0], p.clips[1]);
        assert_eq!((a.start, a.length, a.offset), (960, 1000, 0));
        assert_eq!((b.start, b.length, b.offset), (1960, 2840, 1000));
    }

    #[test]
    fn muted_tracks_and_clips_are_silent() {
        let mut p = Playlist::default();
        p.clips.push(clip(0, 0, 10));
        p.clips.push(clip(1, 0, 10));
        p.clips.push(Clip { muted: true, ..clip(2, 0, 10) });
        p.tracks[1].muted = true;
        assert_eq!(p.audible_clips().count(), 1);
    }
}
