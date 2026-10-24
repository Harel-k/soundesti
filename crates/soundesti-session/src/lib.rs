//! Keeps a running [`Engine`] in sync with a [`Project`].
//!
//! The project is the source of truth. [`Session::sync`] diffs the project
//! against what the engine was last told and sends only the needed commands, so the
//! UI can call it after any edit (or every frame) without flooding the audio thread.

use std::collections::HashMap;

use soundesti_audio::{Command, Engine, EngineConfig, EngineHandle, PlaybackNote, PlaybackSnapshot};
use soundesti_plugin_api::Instrument;
use soundesti_project::{Channel, ChannelId, InstrumentState, Project};
use soundesti_timeline::TempoMap;

/// Creates and configures the instrument described by a channel.
pub fn build_instrument(state: &InstrumentState, sample_rate: f32) -> Option<Box<dyn Instrument>> {
    let mut inst = soundesti_synthesis::create_instrument(&state.type_id, sample_rate)?;
    apply_params(inst.as_mut(), state);
    Some(inst)
}

fn apply_params(inst: &mut dyn Instrument, state: &InstrumentState) {
    for p in inst.params() {
        if let Some(&v) = state.params.get(p.key) {
            inst.set_param(p.id, v);
        }
    }
}

/// Converts the project's current pattern into what the engine plays.
/// `slots` maps channel IDs to engine slots.
pub fn build_snapshot(project: &Project, slots: &HashMap<ChannelId, usize>) -> PlaybackSnapshot {
    let pattern = project.current();
    let ts = project.time_signature;
    let mut notes = Vec::new();
    for pc in &pattern.channels {
        let Some(&slot) = slots.get(&pc.channel) else { continue };
        notes.extend(pc.notes.iter().filter(|n| !n.muted).map(|n| PlaybackNote {
            channel: slot as u16,
            start: n.start,
            end: n.end(),
            key: n.key,
            velocity: n.velocity,
        }));
    }
    PlaybackSnapshot::new(TempoMap::constant(project.bpm), ts, 0, pattern.length_ticks(ts), notes)
}

#[derive(Clone, PartialEq)]
struct ChannelMirror {
    slot: usize,
    instrument: InstrumentState,
    volume: f32,
    pan: f32,
    muted: bool,
    track: u16,
}

#[derive(Clone, Copy, PartialEq)]
struct TrackMirror {
    volume: f32,
    pan: f32,
    muted: bool,
}

pub struct Session {
    handle: EngineHandle,
    sample_rate: f32,
    max_channels: usize,
    channels: HashMap<ChannelId, ChannelMirror>,
    tracks: Vec<Option<TrackMirror>>,
    snapshot_key: Option<SnapshotKey>,
    metronome: Option<bool>,
    /// Soloed channels (UI state, FL does not save channel solo).
    pub solo: Vec<ChannelId>,
    /// Soloed mixer insert: every other insert is muted (the master never is).
    pub mixer_solo: Option<usize>,
}

/// Everything the playback snapshot depends on, to skip rebuilding it when unchanged.
#[derive(Clone, PartialEq)]
struct SnapshotKey {
    pattern: soundesti_project::Pattern,
    bpm: f64,
    ts: soundesti_timeline::TimeSignature,
    slots: Vec<(ChannelId, usize)>,
}

impl Session {
    pub fn new(handle: EngineHandle, config: EngineConfig) -> Self {
        Self {
            handle,
            sample_rate: config.sample_rate as f32,
            max_channels: config.max_channels,
            channels: HashMap::new(),
            tracks: Vec::new(),
            snapshot_key: None,
            metronome: None,
            solo: Vec::new(),
            mixer_solo: None,
        }
    }

    /// Builds an engine plus a session already synced to `project` (offline export, tests).
    pub fn with_engine(project: &Project, config: EngineConfig) -> (Engine, Session) {
        let (engine, handle) = Engine::new(config);
        let mut session = Session::new(handle, config);
        session.sync(project);
        (engine, session)
    }

    pub fn handle(&mut self) -> &mut EngineHandle {
        &mut self.handle
    }

    pub fn send(&mut self, cmd: Command) -> bool {
        self.handle.send(cmd)
    }

    pub fn slot_of(&self, id: ChannelId) -> Option<usize> {
        self.channels.get(&id).map(|c| c.slot)
    }

    fn free_slot(&self) -> Option<usize> {
        (0..self.max_channels).find(|s| !self.channels.values().any(|c| c.slot == *s))
    }

    /// Brings the engine in line with `project`. Cheap when nothing changed.
    pub fn sync(&mut self, project: &Project) {
        self.handle.collect_garbage();

        // Removed channels.
        let live: Vec<ChannelId> = project.channels.iter().map(|c| c.id).collect();
        let gone: Vec<ChannelId> = self.channels.keys().filter(|id| !live.contains(id)).copied().collect();
        for id in gone {
            if let Some(m) = self.channels.remove(&id) {
                self.handle.send(Command::RemoveChannel { slot: m.slot });
            }
        }
        self.solo.retain(|id| live.contains(id));

        // Added / changed channels.
        let any_solo = !self.solo.is_empty();
        for ch in &project.channels {
            let muted = ch.muted || (any_solo && !self.solo.contains(&ch.id));
            self.sync_channel(ch, muted);
        }

        // Mixer tracks.
        self.tracks.resize(project.mixer.len(), None);
        for (i, t) in project.mixer.iter().enumerate() {
            let soloed_out = self.mixer_solo.is_some_and(|s| s != i && i != 0);
            let muted = t.muted || soloed_out;
            let m = TrackMirror { volume: t.volume, pan: t.pan, muted };
            if self.tracks[i] != Some(m) {
                self.handle.send(Command::SetTrack { track: i, volume: t.volume, pan: t.pan, muted });
                self.tracks[i] = Some(m);
            }
        }

        if self.metronome != Some(project.metronome) {
            self.handle.send(Command::SetMetronome(project.metronome));
            self.metronome = Some(project.metronome);
        }

        // Playback snapshot.
        let mut slots: Vec<(ChannelId, usize)> = self.channels.iter().map(|(id, m)| (*id, m.slot)).collect();
        slots.sort();
        let key =
            SnapshotKey { pattern: project.current().clone(), bpm: project.bpm, ts: project.time_signature, slots };
        if self.snapshot_key.as_ref() != Some(&key) {
            let map: HashMap<ChannelId, usize> = key.slots.iter().copied().collect();
            let snap = build_snapshot(project, &map);
            self.handle.send(Command::ReplaceSnapshot(Box::new(snap)));
            self.snapshot_key = Some(key);
        }
    }

    fn sync_channel(&mut self, ch: &Channel, muted: bool) {
        let existing = self.channels.get(&ch.id).cloned();
        let slot = match &existing {
            Some(m) => m.slot,
            None => match self.free_slot() {
                Some(s) => s,
                None => return, // engine full; channel stays silent (reported by UI)
            },
        };
        let needs_new_instrument = existing.as_ref().is_none_or(|m| m.instrument.type_id != ch.instrument.type_id);
        if needs_new_instrument {
            let Some(inst) = build_instrument(&ch.instrument, self.sample_rate) else { return };
            self.handle.send(Command::AddChannel { slot, instrument: inst });
        } else if let Some(m) = &existing
            && m.instrument.params != ch.instrument.params
        {
            for (key, &value) in &ch.instrument.params {
                if m.instrument.params.get(key) != Some(&value)
                    && let Some(id) = param_id(&ch.instrument.type_id, key)
                {
                    self.handle.send(Command::SetParam { slot, id, value });
                }
            }
            // Keys that disappeared (undo, new project) fall back to the parameter default.
            for key in m.instrument.params.keys().filter(|k| !ch.instrument.params.contains_key(*k)) {
                if let Some(p) = instrument_params(&ch.instrument.type_id).iter().find(|p| p.key == key) {
                    self.handle.send(Command::SetParam { slot, id: p.id, value: p.default });
                }
            }
        }
        let mirror = ChannelMirror {
            slot,
            instrument: ch.instrument.clone(),
            volume: ch.volume,
            pan: ch.pan,
            muted,
            track: ch.mixer_track,
        };
        let mixer_changed = existing.as_ref().is_none_or(|m| {
            (m.volume, m.pan, m.muted, m.track) != (mirror.volume, mirror.pan, mirror.muted, mirror.track)
        });
        if needs_new_instrument || mixer_changed {
            self.handle.send(Command::SetChannel {
                slot,
                volume: ch.volume,
                pan: ch.pan,
                muted,
                track: ch.mixer_track as usize,
            });
        }
        self.channels.insert(ch.id, mirror);
    }
}

/// Looks up a parameter's numeric ID from its persistent key.
pub fn param_id(type_id: &str, key: &str) -> Option<u32> {
    instrument_params(type_id).iter().find(|p| p.key == key).map(|p| p.id)
}

/// Static parameter list for an instrument type (for building editor UIs).
pub fn instrument_params(type_id: &str) -> &'static [soundesti_plugin_api::ParamInfo] {
    match type_id {
        soundesti_synthesis::trident::TYPE_ID => &soundesti_synthesis::trident::PARAMS,
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_project::Note;

    const CFG: EngineConfig = EngineConfig { sample_rate: 48_000.0, max_block: 512, max_channels: 16 };

    fn render(engine: &mut Engine, frames: usize) -> Vec<f32> {
        let mut out = vec![0.0; frames * 2];
        for c in out.chunks_mut(1024) {
            engine.process(c, 2);
        }
        out.chunks(2).map(|f| f[0]).collect()
    }

    #[test]
    fn steps_in_project_produce_audio() {
        let mut p = Project::new();
        let ch = p.channels[0].id;
        p.current_mut().toggle_step(ch, 0);
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        session.send(Command::Play);
        let left = render(&mut engine, 9_600);
        assert!(left.iter().any(|x| x.abs() > 1e-3), "step did not sound");
    }

    #[test]
    fn sync_is_incremental() {
        let mut p = Project::new();
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        render(&mut engine, 512);
        assert_eq!(session.handle().collect_garbage(), 1, "initial empty snapshot is replaced");
        session.sync(&p);
        // Nothing changed: no new snapshot is sent, so nothing comes back.
        render(&mut engine, 512);
        assert_eq!(session.handle().collect_garbage(), 0, "unchanged project must not resend");
        p.bpm = 90.0;
        session.sync(&p);
        render(&mut engine, 512);
        assert_eq!(session.handle().collect_garbage(), 1, "tempo change replaces one snapshot");
    }

    #[test]
    fn param_changes_reach_the_engine() {
        let mut p = Project::new();
        let ch = p.channels[0].id;
        p.current_mut().notes_mut(ch).push(Note::new(0, 3840, 60));
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        session.send(Command::Play);
        let loud = render(&mut engine, 4_800);
        p.channels[0].instrument.params.insert("master.volume".into(), 0.0);
        session.sync(&p);
        let quiet = render(&mut engine, 4_800);
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |a, x| a.max(x.abs()));
        assert!(peak(&loud) > 0.01);
        assert_eq!(peak(&quiet[512..]), 0.0, "volume 0 must silence Trident");
    }

    #[test]
    fn removed_param_resets_to_default() {
        let mut p = Project::new();
        let ch = p.channels[0].id;
        p.current_mut().notes_mut(ch).push(Note::new(0, 3840, 60));
        p.channels[0].instrument.params.insert("master.volume".into(), 0.0);
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        session.send(Command::Play);
        let silent = render(&mut engine, 4_800);
        assert!(silent.iter().all(|&x| x == 0.0));
        p.channels[0].instrument.params.clear(); // e.g. undo back to defaults
        session.sync(&p);
        let loud = render(&mut engine, 4_800);
        assert!(loud.iter().any(|x| x.abs() > 1e-3), "volume must return to its default");
    }

    #[test]
    fn solo_mutes_other_channels() {
        let mut p = Project::new();
        let a = p.channels[0].id;
        let b = p.add_channel("B", "soundesti.trident");
        p.current_mut().notes_mut(a).push(Note::new(0, 3840, 60));
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        session.solo.push(b);
        session.sync(&p);
        session.send(Command::Play);
        let left = render(&mut engine, 9_600);
        assert!(left[1_000..].iter().all(|&x| x == 0.0), "soloing B must mute A");
    }

    #[test]
    fn removing_a_channel_frees_its_slot() {
        let mut p = Project::new();
        let b = p.add_channel("B", "soundesti.trident");
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        let slot_b = session.slot_of(b).unwrap();
        p.remove_channel(b);
        session.sync(&p);
        assert!(session.slot_of(b).is_none());
        let c = p.add_channel("C", "soundesti.trident");
        session.sync(&p);
        assert_eq!(session.slot_of(c), Some(slot_b));
        render(&mut engine, 512);
    }
}
