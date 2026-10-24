//! Keeps a running [`Engine`] in sync with a [`Project`].
//!
//! The project is the source of truth. [`Session::sync`] diffs the project
//! against what the engine was last told and sends only the needed commands, so the
//! UI can call it after any edit (or every frame) without flooding the audio thread.

pub mod samples;

use std::collections::HashMap;
use std::sync::Arc;

use soundesti_audio::{Command, Engine, EngineConfig, EngineHandle, PlaybackNote, PlaybackSnapshot};
use soundesti_plugin_api::Instrument;
use soundesti_plugin_host::PluginInstance;
use soundesti_project::{Channel, ChannelId, InstrumentState, PLUGIN_TYPE_ID, PluginRef, Project};
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

/// Converts the project into what the engine plays: the whole Playlist in song mode,
/// otherwise the current pattern on loop. `slots` maps channel IDs to engine slots.
pub fn build_snapshot(project: &Project, slots: &HashMap<ChannelId, usize>) -> PlaybackSnapshot {
    let ts = project.time_signature;
    if project.song_mode {
        let mut notes = Vec::new();
        for clip in project.playlist.audible_clips() {
            let Some(pattern) = project.pattern(clip.pattern) else { continue };
            for pc in &pattern.channels {
                let Some(&slot) = slots.get(&pc.channel) else { continue };
                for n in pc.notes.iter().filter(|n| !n.muted) {
                    // Position inside the clip window [offset, offset + length).
                    let rel = n.start - clip.offset;
                    if rel < 0 || rel >= clip.length {
                        continue;
                    }
                    let start = clip.start + rel;
                    notes.push(PlaybackNote {
                        channel: slot as u16,
                        start,
                        end: (start + n.length).min(clip.end()),
                        key: n.key,
                        velocity: n.velocity,
                    });
                }
            }
        }
        return PlaybackSnapshot::new(TempoMap::constant(project.bpm), ts, 0, project.playlist.song_length(ts), notes);
    }
    let pattern = project.current();
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
    /// Last effect chain sent per mixer track.
    fx: Vec<Vec<soundesti_project::EffectSlot>>,
    snapshot_key: Option<SnapshotKey>,
    metronome: Option<bool>,
    /// Soloed channels (UI state, FL does not save channel solo).
    pub solo: Vec<ChannelId>,
    /// Soloed mixer insert: every other insert is muted (the master never is).
    pub mixer_solo: Option<usize>,
    /// Decoded samples by path (shared by channels using the same file).
    sample_cache: HashMap<String, Arc<soundesti_plugin_api::SampleData>>,
    /// Last sample that failed to load, for the UI to report.
    pub sample_error: Option<String>,
    /// The Browser's preview voice lives in the last engine slot (never used by channels).
    preview_ready: bool,
    /// Running third-party plugins, for their editor windows and saving their settings.
    plugins: HashMap<PluginKey, Arc<dyn PluginInstance>>,
    /// Plugins that failed to load since the UI last looked.
    pub plugin_errors: Vec<String>,
}

/// Where a running plugin sits in the project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PluginKey {
    Channel(ChannelId),
    /// Mixer track, effect slot.
    Effect(usize, usize),
}

/// Stand-in for a plugin that could not be loaded: the channel stays, silent.
struct Silent;

impl soundesti_plugin_api::Instrument for Silent {
    fn type_id(&self) -> &'static str {
        PLUGIN_TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Missing plugin"
    }
    fn params(&self) -> &'static [soundesti_plugin_api::ParamInfo] {
        &[]
    }
    fn get_param(&self, _id: u32) -> f32 {
        0.0
    }
    fn set_param(&mut self, _id: u32, _value: f32) {}
    fn process(&mut self, _events: &[soundesti_plugin_api::TimedEvent], _l: &mut [f32], _r: &mut [f32]) {}
    fn active_voices(&self) -> usize {
        0
    }
}

/// Turns a project plugin reference into what the host needs to load it.
pub fn plugin_desc(r: &PluginRef) -> Option<soundesti_plugin_host::PluginDesc> {
    use soundesti_plugin_host::PluginFormat;
    let format = match r.format.as_str() {
        "vst2" => PluginFormat::Vst2,
        "vst3" => PluginFormat::Vst3,
        "clap" => PluginFormat::Clap,
        _ => return None,
    };
    Some(soundesti_plugin_host::PluginDesc {
        format,
        path: r.path.clone().into(),
        id: r.id.clone(),
        name: r.name.clone(),
        vendor: r.vendor.clone(),
        is_instrument: false,
        inputs: 0,
        outputs: 0,
    })
}

/// The project reference for a scanned plugin.
pub fn plugin_ref(d: &soundesti_plugin_host::PluginDesc) -> PluginRef {
    let format = match d.format {
        soundesti_plugin_host::PluginFormat::Vst2 => "vst2",
        soundesti_plugin_host::PluginFormat::Vst3 => "vst3",
        soundesti_plugin_host::PluginFormat::Clap => "clap",
    };
    PluginRef {
        format: format.into(),
        path: d.path.display().to_string(),
        id: d.id.clone(),
        name: d.name.clone(),
        vendor: d.vendor.clone(),
        state: None,
    }
}

fn same_plugin(a: Option<&PluginRef>, b: Option<&PluginRef>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.same_plugin(b),
        (None, None) => true,
        _ => false,
    }
}

/// MIDI key the preview sampler plays (its root key, so samples play at their own pitch).
const PREVIEW_KEY: u8 = 60;

/// Everything the playback snapshot depends on, to skip rebuilding it when unchanged.
#[derive(Clone, PartialEq)]
struct SnapshotKey {
    /// Current pattern in pattern mode; every pattern plus the playlist in song mode.
    patterns: Vec<soundesti_project::Pattern>,
    playlist: Option<soundesti_project::Playlist>,
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
            fx: Vec::new(),
            snapshot_key: None,
            metronome: None,
            solo: Vec::new(),
            mixer_solo: None,
            sample_cache: HashMap::new(),
            sample_error: None,
            preview_ready: false,
            plugins: HashMap::new(),
            plugin_errors: Vec::new(),
        }
    }

    /// Gives every running plugin its regular main-thread call (call once per UI frame).
    pub fn idle_plugins(&self) {
        for p in self.plugins.values() {
            p.main_thread_idle();
        }
    }

    /// The running plugin at `key`, if any (for opening its editor).
    pub fn plugin(&self, key: PluginKey) -> Option<Arc<dyn PluginInstance>> {
        self.plugins.get(&key).cloned()
    }

    /// Loads a plugin and restores its saved settings. Failures are reported, not fatal.
    fn load_plugin(&mut self, key: PluginKey, r: &PluginRef) -> Option<Arc<dyn PluginInstance>> {
        let loaded = plugin_desc(r)
            .ok_or_else(|| format!("unknown plugin format \"{}\"", r.format))
            .and_then(|d| soundesti_plugin_host::load(&d, self.sample_rate).map_err(|e| e.to_string()));
        match loaded {
            Ok(p) => {
                if let Some(state) = r.state.as_deref().and_then(soundesti_project::b64::decode)
                    && !p.load_state(&state)
                {
                    self.plugin_errors.push(format!("{}: saved settings could not be restored", r.name));
                }
                self.plugins.insert(key, p.clone());
                Some(p)
            }
            Err(e) => {
                self.plugins.remove(&key);
                self.plugin_errors.push(format!("{}: {e}", r.name));
                None
            }
        }
    }

    /// Copies every running plugin's current settings into the project (before saving).
    pub fn capture_plugin_states(&self, project: &mut Project) {
        for ch in &mut project.channels {
            if let (Some(r), Some(p)) = (ch.instrument.plugin.as_mut(), self.plugins.get(&PluginKey::Channel(ch.id))) {
                r.state = p.save_state().map(|s| soundesti_project::b64::encode(&s));
            }
        }
        for (t, track) in project.mixer.iter_mut().enumerate() {
            for e in &mut track.effects {
                if let (Some(r), Some(p)) =
                    (e.plugin.as_mut(), self.plugins.get(&PluginKey::Effect(t, e.slot as usize)))
                {
                    r.state = p.save_state().map(|s| soundesti_project::b64::encode(&s));
                }
            }
        }
    }

    fn preview_slot(&self) -> usize {
        self.max_channels - 1
    }

    /// Plays a sample once through the master (the Browser's click-to-preview).
    pub fn preview_sample(&mut self, path: &str) -> bool {
        let Some(sample) = self.load_sample(path) else { return false };
        let slot = self.preview_slot();
        if !self.preview_ready {
            let Some(inst) =
                soundesti_synthesis::create_instrument(soundesti_synthesis::sampler::TYPE_ID, self.sample_rate)
            else {
                return false;
            };
            self.handle.send(Command::AddChannel { slot, instrument: inst });
            self.handle.send(Command::SetChannel { slot, volume: 0.78, pan: 0.0, muted: false, track: 0 });
            self.preview_ready = true;
        }
        // Replacing the sample also stops the previous preview.
        self.handle.send(Command::SetSample { slot, sample: Some(sample) });
        self.handle.send(Command::NoteOn { slot, key: PREVIEW_KEY, velocity: 0.8 });
        true
    }

    pub fn stop_preview(&mut self) {
        if self.preview_ready {
            let slot = self.preview_slot();
            self.handle.send(Command::SetSample { slot, sample: None });
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
        (0..self.max_channels - 1).find(|s| !self.channels.values().any(|c| c.slot == *s))
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
            self.plugins.remove(&PluginKey::Channel(id));
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
        self.fx.resize(project.mixer.len(), Vec::new());
        for (i, t) in project.mixer.iter().enumerate() {
            if self.fx[i] != t.effects {
                self.sync_effects(i, &t.effects);
            }
        }

        if self.metronome != Some(project.metronome) {
            self.handle.send(Command::SetMetronome(project.metronome));
            self.metronome = Some(project.metronome);
        }

        // Playback snapshot.
        let mut slots: Vec<(ChannelId, usize)> = self.channels.iter().map(|(id, m)| (*id, m.slot)).collect();
        slots.sort();
        let key = SnapshotKey {
            patterns: if project.song_mode { project.patterns.clone() } else { vec![project.current().clone()] },
            playlist: project.song_mode.then(|| project.playlist.clone()),
            bpm: project.bpm,
            ts: project.time_signature,
            slots,
        };
        if self.snapshot_key.as_ref() != Some(&key) {
            let map: HashMap<ChannelId, usize> = key.slots.iter().copied().collect();
            let snap = build_snapshot(project, &map);
            self.handle.send(Command::ReplaceSnapshot(Box::new(snap)));
            self.snapshot_key = Some(key);
        }
    }

    /// Sends the difference between the last-sent chain of `track` and `effects`.
    fn sync_effects(&mut self, track: usize, effects: &[soundesti_project::EffectSlot]) {
        let old = std::mem::take(&mut self.fx[track]);
        for slot in 0..soundesti_project::FX_SLOTS {
            let before = old.iter().find(|e| e.slot as usize == slot);
            let after = effects.iter().find(|e| e.slot as usize == slot);
            match (before, after) {
                (None, None) => {}
                (Some(_), None) => {
                    self.plugins.remove(&PluginKey::Effect(track, slot));
                    self.handle.send(Command::SetEffect { track, slot, effect: None });
                }
                (b, Some(a))
                    if b.is_none_or(|b| {
                        b.type_id != a.type_id || !same_plugin(b.plugin.as_ref(), a.plugin.as_ref())
                    }) && a.type_id == PLUGIN_TYPE_ID =>
                {
                    let plugin = a.plugin.as_ref().and_then(|r| self.load_plugin(PluginKey::Effect(track, slot), r));
                    let effect = plugin.map(|p| {
                        Box::new(soundesti_plugin_host::PluginEffect { plugin: p })
                            as Box<dyn soundesti_plugin_api::Effect>
                    });
                    self.handle.send(Command::SetEffect { track, slot, effect });
                    self.handle.send(Command::SetEffectBypass { track, slot, bypass: a.bypass });
                }
                (b, Some(a)) => {
                    let infos = soundesti_effects::params_for(&a.type_id);
                    if b.is_none_or(|b| b.type_id != a.type_id) {
                        self.plugins.remove(&PluginKey::Effect(track, slot));
                        let effect = soundesti_effects::create(&a.type_id, self.sample_rate).map(|mut fx| {
                            for p in infos {
                                if let Some(&v) = a.params.get(p.key) {
                                    fx.set_param(p.id, v);
                                }
                            }
                            fx
                        });
                        self.handle.send(Command::SetEffect { track, slot, effect });
                        self.handle.send(Command::SetEffectBypass { track, slot, bypass: a.bypass });
                        continue;
                    }
                    let b = b.expect("checked above");
                    for p in infos {
                        let (old_v, new_v) = (b.params.get(p.key), a.params.get(p.key));
                        if old_v != new_v {
                            let value = new_v.copied().unwrap_or(p.default);
                            self.handle.send(Command::SetEffectParam { track, slot, id: p.id, value });
                        }
                    }
                    if b.bypass != a.bypass {
                        self.handle.send(Command::SetEffectBypass { track, slot, bypass: a.bypass });
                    }
                }
            }
        }
        self.fx[track] = effects.to_vec();
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
        let needs_new_instrument = existing.as_ref().is_none_or(|m| {
            m.instrument.type_id != ch.instrument.type_id
                || !same_plugin(m.instrument.plugin.as_ref(), ch.instrument.plugin.as_ref())
        });
        if needs_new_instrument {
            let inst: Box<dyn Instrument> = if ch.instrument.type_id == PLUGIN_TYPE_ID {
                let plugin = ch.instrument.plugin.as_ref().and_then(|r| self.load_plugin(PluginKey::Channel(ch.id), r));
                match plugin {
                    Some(p) => Box::new(soundesti_plugin_host::PluginInstrument::new(p)),
                    None => Box::new(Silent),
                }
            } else {
                self.plugins.remove(&PluginKey::Channel(ch.id));
                let Some(inst) = build_instrument(&ch.instrument, self.sample_rate) else { return };
                inst
            };
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
        let sample_changed =
            needs_new_instrument || existing.as_ref().is_some_and(|m| m.instrument.sample != ch.instrument.sample);
        if sample_changed {
            let data = ch.instrument.sample.as_deref().and_then(|p| self.load_sample(p));
            if data.is_some() || !needs_new_instrument {
                self.handle.send(Command::SetSample { slot, sample: data });
            }
        }
        let pads_changed =
            needs_new_instrument || existing.as_ref().is_some_and(|m| m.instrument.pads != ch.instrument.pads);
        if pads_changed {
            let before = existing.as_ref().map(|m| m.instrument.pads.clone()).unwrap_or_default();
            let n = ch.instrument.pads.len().max(if needs_new_instrument { 0 } else { before.len() });
            for index in 0..n {
                let now = ch.instrument.pads.get(index).cloned().flatten();
                let was = before.get(index).cloned().flatten();
                if needs_new_instrument || now != was {
                    let data = now.as_deref().and_then(|p| self.load_sample(p));
                    self.handle.send(Command::SetSampleAt { slot, index, sample: data });
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

    /// Decodes (or reuses) a sample file; records a readable error on failure.
    pub fn load_sample(&mut self, path: &str) -> Option<Arc<soundesti_plugin_api::SampleData>> {
        if let Some(s) = self.sample_cache.get(path) {
            return Some(s.clone());
        }
        match samples::load_wav(std::path::Path::new(path)) {
            Ok(s) => {
                self.sample_cache.insert(path.to_string(), s.clone());
                Some(s)
            }
            Err(e) => {
                self.sample_error = Some(e);
                None
            }
        }
    }
}

/// Looks up a parameter's numeric ID from its persistent key.
pub fn param_id(type_id: &str, key: &str) -> Option<u32> {
    instrument_params(type_id).iter().find(|p| p.key == key).map(|p| p.id)
}

/// Static parameter list for an instrument type (for building editor UIs).
pub fn instrument_params(type_id: &str) -> &'static [soundesti_plugin_api::ParamInfo] {
    soundesti_synthesis::params_for(type_id)
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
    fn song_mode_plays_playlist_clips() {
        // 120 BPM: one 4/4 bar = 2 s = 96 000 samples.
        let mut p = Project::new();
        p.bpm = 120.0;
        let ch = p.channels[0].id;
        p.current_mut().toggle_step(ch, 0);
        let pid = p.current_pattern;
        p.song_mode = true;
        for bar in [0, 2] {
            p.playlist.clips.push(soundesti_project::Clip {
                pattern: pid,
                track: 0,
                start: bar * 3840,
                length: 3840,
                offset: 0,
                muted: false,
                selected: false,
            });
        }
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        session.send(Command::Play);
        let left = render(&mut engine, 96_000 * 3);
        let loud = |r: std::ops::Range<usize>| left[r].iter().any(|x| x.abs() > 1e-3);
        assert!(loud(0..9_600), "clip at bar 1 must play");
        assert!(!loud(96_000 + 48_000..96_000 * 2), "bar 2 has no clip");
        assert!(loud(96_000 * 2..96_000 * 2 + 9_600), "clip at bar 3 must play");
    }

    #[test]
    fn missing_plugins_stay_silent_and_report_once() {
        let mut p = Project::empty();
        let ch = p.add_channel("Gone synth", PLUGIN_TYPE_ID);
        p.channel_mut(ch).unwrap().instrument.plugin = Some(PluginRef {
            format: "vst2".into(),
            path: "C:\\nowhere\\Gone.dll".into(),
            id: "00000001".into(),
            name: "Gone".into(),
            vendor: String::new(),
            state: None,
        });
        p.current_mut().notes_mut(ch).push(Note::new(0, 960, 60));
        p.mixer[0].set_plugin_effect(
            0,
            PluginRef {
                format: "vst3".into(),
                path: "C:\\nowhere\\Fx.vst3".into(),
                id: "x".into(),
                name: "Fx".into(),
                vendor: String::new(),
                state: None,
            },
        );
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        assert_eq!(session.plugin_errors.len(), 2, "{:?}", session.plugin_errors);
        assert!(session.plugin_errors[0].starts_with("Gone:"));
        session.send(Command::Play);
        assert!(render(&mut engine, 4_800).iter().all(|x| *x == 0.0), "missing plugin is silent");
        session.plugin_errors.clear();
        session.sync(&p);
        assert!(session.plugin_errors.is_empty(), "no retry on every sync");
        assert!(session.plugin(PluginKey::Channel(ch)).is_none());
    }

    #[test]
    fn browser_preview_plays_a_pack_sample() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/packs/Soundesti Essentials/Kicks/SE Kick Punch.wav");
        let p = Project::empty();
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        assert!(render(&mut engine, 2_048).iter().all(|x| *x == 0.0), "empty project is silent");
        assert!(session.preview_sample(&path.display().to_string()), "pack sample loads");
        let out = render(&mut engine, 9_600);
        assert!(out.iter().any(|x| x.abs() > 0.05), "preview is audible without pressing play");
        session.stop_preview();
        render(&mut engine, 1_024);
        assert!(render(&mut engine, 2_048).iter().all(|x| x.abs() < 1e-6), "stop silences the preview");
        assert!(!session.preview_sample("missing.wav"));
    }

    #[test]
    fn mixer_effects_follow_the_project() {
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let mut p = Project::new();
        let ch = p.channels[0].id;
        p.current_mut().notes_mut(ch).push(Note::new(0, 3840 * 4, 60));
        let (mut engine, mut session) = Session::with_engine(&p, CFG);
        session.send(Command::Play);
        let dry = peak(&render(&mut engine, 4_800));
        assert!(dry > 1e-3);
        // Utility at -24 dB on the master.
        p.mixer[0].set_effect(2, soundesti_effects::utility::TYPE_ID);
        p.mixer[0].effect_mut(2).unwrap().params.insert("util.gain".into(), -24.0);
        session.sync(&p);
        let quiet = peak(&render(&mut engine, 4_800)[1_000..]);
        assert!(quiet < dry * 0.2, "-24 dB utility: {quiet} vs {dry}");
        p.mixer[0].effect_mut(2).unwrap().bypass = true;
        session.sync(&p);
        let back = peak(&render(&mut engine, 4_800)[1_000..]);
        assert!(back > quiet * 3.0, "bypass restores level");
        p.mixer[0].remove_effect(2);
        session.sync(&p);
        render(&mut engine, 512);
        assert!(session.handle().collect_garbage() >= 1, "removed effect is freed");
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
