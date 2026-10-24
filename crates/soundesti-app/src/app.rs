//! Application state, keyboard dispatch and the main frame.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use egui::{Color32, LayerId, RichText, Ui};
use soundesti_audio::{Command as EngineCmd, Shared};
use soundesti_audio_device::{AudioOutput, OutputSettings};
use soundesti_commands::Command;
use soundesti_project::{ChannelId, History, Project};
use soundesti_session::Session;
use soundesti_shortcuts::{Context, InputState, Keymap, typing_piano_offset};
use soundesti_timeline::{BarStepTick, PPQ, TICKS_PER_STEP, Tick};

use crate::files::ExportJob;
use crate::input::chord_from_egui;
use crate::piano_roll::PianoRoll;
use crate::widgets::MeterState;
use crate::{icons, theme};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Draw,
    Paint,
    Delete,
    Mute,
    Slice,
    Select,
    Zoom,
}

impl Tool {
    pub const ALL: [Tool; 7] =
        [Tool::Draw, Tool::Paint, Tool::Delete, Tool::Mute, Tool::Slice, Tool::Select, Tool::Zoom];

    pub fn label(self) -> &'static str {
        match self {
            Tool::Draw => "Draw (P)",
            Tool::Paint => "Paint (B)",
            Tool::Delete => "Delete (D)",
            Tool::Mute => "Mute (T)",
            Tool::Slice => "Slice (C)",
            Tool::Select => "Select (E)",
            Tool::Zoom => "Zoom (Z)",
        }
    }

    pub fn icon(self) -> icons::Icon {
        match self {
            Tool::Draw => icons::Icon::Pencil,
            Tool::Paint => icons::Icon::Brush,
            Tool::Delete => icons::Icon::Erase,
            Tool::Mute => icons::Icon::Mute,
            Tool::Slice => icons::Icon::Slice,
            Tool::Select => icons::Icon::Select,
            Tool::Zoom => icons::Icon::Zoom,
        }
    }
}

/// Snap choices (ticks; 0 = none). Labels follow FL's naming style.
pub const SNAPS: &[(&str, Tick)] = &[
    ("(none)", 0),
    ("1/6 step", TICKS_PER_STEP / 6),
    ("1/4 step", TICKS_PER_STEP / 4),
    ("1/3 step", TICKS_PER_STEP / 3),
    ("1/2 step", TICKS_PER_STEP / 2),
    ("Step", TICKS_PER_STEP),
    ("1/3 beat", PPQ / 3),
    ("1/2 beat", PPQ / 2),
    ("Beat", PPQ),
    ("Bar", PPQ * 4),
];

#[derive(Default)]
pub struct Windows {
    pub channel_rack: bool,
    pub piano_roll: bool,
    pub mixer: bool,
    pub playlist: bool,
    pub browser: bool,
    pub plugin_picker: bool,
    pub song_info: bool,
    pub midi_settings: bool,
    pub audio_settings: bool,
    pub help: bool,
    pub quantize: bool,
    pub export: bool,
    pub ai_setup: bool,
    pub instruments: BTreeSet<ChannelId>,
    /// Open effect windows as (mixer track, slot).
    pub effects: BTreeSet<(usize, usize)>,
    pub plugin_manager: bool,
}

pub enum Dialog {
    RenamePattern { id: soundesti_project::PatternId, text: String },
    RenameChannel { id: ChannelId, text: String },
    RenameTrack { index: usize, text: String },
    ConfirmDiscard { then: AfterConfirm },
    Message { title: String, text: String },
}

#[derive(Clone)]
pub enum AfterConfirm {
    New,
    Open(Option<PathBuf>),
    /// File › Import › FL Studio project.
    ImportFlp,
    Quit,
    /// Quit, then run the downloaded installer in update mode.
    QuitAndUpdate(PathBuf),
}

pub struct App {
    pub project: Project,
    pub history: History,
    pub revision: u64,
    pub saved_revision: u64,
    pub path: Option<PathBuf>,
    pub keymap: Keymap,
    pub focus: Context,
    pub typing_piano: bool,
    pub typing_octave: i32,
    typing_held: HashMap<soundesti_shortcuts::Key, (usize, u8)>,

    pub output: AudioOutput,
    pub session: Session,
    pub shared: Arc<Shared>,
    pub audio_settings: OutputSettings,
    pub device_names: Vec<String>,
    pub audio_error: Option<String>,

    pub win: Windows,
    window_layers: HashMap<LayerId, Context>,
    /// Docked areas (Browser, Playlist) from the last frame, for click-to-focus.
    docked: Vec<(egui::Rect, Context)>,
    /// Area between the docked Browser and the screen edges; floating windows stay inside it.
    pub workspace: egui::Rect,
    pub selected_channel: Option<ChannelId>,
    pub selected_channels: BTreeSet<ChannelId>,
    pub piano: PianoRoll,
    pub playlist: crate::playlist::PlaylistView,
    pub browser: crate::browser::BrowserView,
    pub plugins: crate::plugins::PluginUi,
    pub live: crate::live::LiveInput,
    /// Plugin editor to open once the next sync has started the plugin.
    pub pending_editor: Option<soundesti_session::PluginKey>,
    pub selected_track: usize,
    pub track_meters: Vec<MeterState>,
    pub channel_meters: Vec<MeterState>,
    pub mixer_solo: Option<usize>,
    pub snap: Tick,
    snap_before_none: Tick,
    pub step_clipboard: Option<Vec<soundesti_project::Note>>,

    pub status: String,
    pending: Vec<(Command, Option<i32>)>,
    pub dialog: Option<Dialog>,
    pub export_job: Option<ExportJob>,
    pub recent: Vec<PathBuf>,
    pub last_autosave: Instant,
    pub needs_sync: bool,
    pub quit_confirmed: bool,
    last_frame: Instant,
    pub peak_cpu: f32,
    pub preview: Option<(usize, u8)>,
    pub quantize_strength: f32,
    pub quantize_ends: bool,
    pub help_query: String,
    pub export_opts: soundesti_export::ExportOptions,
    /// Render one file per mixer track (FL's `Split mixer tracks`).
    pub export_stems: bool,
    /// Action waiting to run after the unsaved-changes prompt.
    pub after: Option<AfterConfirm>,
    pub rack_paint: Option<crate::channel_rack::StepPaint>,
    started: bool,
    pub update: crate::updates::UpdateState,
    pub auto_update: bool,
    /// Built-in MCP server for AI control (None = off).
    pub mcp: Option<crate::mcp::McpServer>,
    pub mcp_enabled: bool,
    pub ai_setup: crate::ai_setup::AiSetupState,
}

pub const MCP_KEY: &str = "soundesti.mcp";

pub const RECENT_KEY: &str = "soundesti.recent";
pub const AUDIO_KEY: &str = "soundesti.audio";

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, open_file: Option<PathBuf>, force_mcp: bool) -> Self {
        let mcp_enabled = force_mcp || cc.storage.and_then(|s| s.get_string(MCP_KEY)).is_some_and(|v| v == "true");
        theme::apply(&cc.egui_ctx);
        let auto_update =
            cc.storage.and_then(|s| s.get_string(crate::updates::AUTO_CHECK_KEY)).is_none_or(|v| v != "false");
        let recent: Vec<PathBuf> = cc
            .storage
            .and_then(|s| s.get_string(RECENT_KEY))
            .and_then(|s| serde_json::from_str::<Vec<String>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .map(PathBuf::from)
            .collect();
        let audio_settings: OutputSettings = cc
            .storage
            .and_then(|s| s.get_string(AUDIO_KEY))
            .and_then(|s| serde_json::from_str::<(Option<String>, Option<u32>, Option<u32>)>(&s).ok())
            .map(|(device, sample_rate, buffer_frames)| OutputSettings { device, sample_rate, buffer_frames })
            .unwrap_or_default();

        let (output, handle, err) = soundesti_audio_device::start_or_fallback(&audio_settings);
        let shared = handle.shared().clone();
        let mut session = Session::new(handle, output.engine_config);
        let project = Project::new();
        session.sync(&project);
        let selected = project.channels.first().map(|c| c.id);

        let mut app = Self {
            history: History::default(),
            revision: 0,
            saved_revision: 0,
            path: None,
            keymap: Keymap::fl_studio_workflow(),
            focus: Context::ChannelRack,
            typing_piano: false,
            typing_octave: 5,
            typing_held: HashMap::new(),
            output,
            session,
            shared,
            audio_settings,
            device_names: soundesti_audio_device::list_output_devices().into_iter().map(|d| d.name).collect(),
            audio_error: err.map(|e| e.to_string()),
            // FL's default layout: Playlist in the background, Channel Rack floating on top.
            win: Windows { channel_rack: true, playlist: true, browser: true, ..Default::default() },
            window_layers: HashMap::new(),
            docked: Vec::new(),
            workspace: egui::Rect::from_min_size(egui::pos2(0.0, 80.0), egui::vec2(1600.0, 800.0)),
            selected_channel: selected,
            selected_channels: selected.into_iter().collect(),
            piano: PianoRoll::default(),
            playlist: Default::default(),
            browser: crate::browser::BrowserView::new(),
            plugins: crate::plugins::PluginUi::new(crate::plugins::main_window_handle(cc)),
            pending_editor: None,
            live: crate::live::LiveInput::new(
                cc.storage.and_then(|s| s.get_string(crate::live::MIDI_PORT_KEY)).filter(|p| !p.is_empty()),
            ),
            selected_track: 1,
            track_meters: vec![MeterState::default(); soundesti_audio::MIXER_TRACKS],
            channel_meters: vec![MeterState::default(); 256],
            mixer_solo: None,
            snap: TICKS_PER_STEP,
            snap_before_none: TICKS_PER_STEP,
            step_clipboard: None,
            status: "Welcome to Soundesti. F6 Channel Rack · F7 Piano roll · F9 Mixer · F1 shortcuts.".into(),
            pending: Vec::new(),
            dialog: None,
            export_job: None,
            recent,
            last_autosave: Instant::now(),
            needs_sync: false,
            quit_confirmed: false,
            last_frame: Instant::now(),
            peak_cpu: 0.0,
            preview: None,
            quantize_strength: 1.0,
            quantize_ends: false,
            help_query: String::new(),
            export_opts: soundesti_export::ExportOptions::default(),
            export_stems: false,
            after: None,
            rack_paint: None,
            started: false,
            update: crate::updates::UpdateState::Idle,
            auto_update,
            mcp: None,
            mcp_enabled: false,
            ai_setup: Default::default(),
            project,
        };
        if mcp_enabled {
            app.set_mcp(&cc.egui_ctx, true);
        }
        if let Some(e) = &app.audio_error {
            app.status = format!("Audio device problem: {e}. Using {}.", app.output.info.device);
        }
        // Double-clicked .soundesti file (file association passes it as the first argument).
        if let Some(p) = open_file {
            app.after = Some(AfterConfirm::Open(Some(p)));
        }
        if app.auto_update {
            app.check_for_updates(false);
        }
        // Reconnect the MIDI keyboard used last time (silently skipped if unplugged).
        if let Some(port) = app.live.midi_port.clone()
            && crate::live::midi_ports().contains(&port)
        {
            app.connect_midi(&cc.egui_ctx, Some(port));
        }
        app
    }

    /// Turns the built-in MCP server on or off.
    pub fn set_mcp(&mut self, ctx: &egui::Context, on: bool) {
        self.mcp = None;
        self.mcp_enabled = on;
        if !on {
            self.set_status("AI control (MCP server) is off.");
            return;
        }
        let port =
            std::env::var("SOUNDESTI_MCP_PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(crate::mcp::DEFAULT_PORT);
        let wake = ctx.clone();
        match crate::mcp::McpServer::start(port, move || wake.request_repaint()) {
            Ok(srv) => {
                self.set_status(format!("AI control on: MCP server at http://127.0.0.1:{port}/mcp (local only)."));
                self.mcp = Some(srv);
            }
            Err(e) => {
                self.mcp_enabled = false;
                self.set_status(format!("Could not start the MCP server on port {port}: {e}"));
            }
        }
    }

    /// Command execution for the MCP server; returns false if not implemented.
    pub fn execute_pub(&mut self, ctx: &egui::Context, cmd: Command, arg: Option<i32>) -> bool {
        self.execute(ctx, cmd, arg)
    }

    // ----- model helpers -------------------------------------------------

    /// Records the current state as an undo step before an edit.
    pub fn checkpoint(&mut self, label: &str) {
        self.history.record(&self.project, label);
        self.revision += 1;
        self.needs_sync = true;
    }

    /// Like [`checkpoint`] but from a state captured earlier (lazy drag checkpoints).
    pub fn checkpoint_from(&mut self, before: &Project, label: &str) {
        self.history.record(before, label);
        self.revision += 1;
        self.needs_sync = true;
    }

    pub fn dirty(&self) -> bool {
        self.revision != self.saved_revision
    }

    pub fn set_status(&mut self, s: impl Into<String>) {
        self.status = s.into();
    }

    pub fn preview_on(&mut self, ch: ChannelId, key: u8, velocity: f32) {
        self.preview_off();
        if let Some(slot) = self.session.slot_of(ch) {
            self.session.send(EngineCmd::NoteOn { slot, key, velocity });
            self.preview = Some((slot, key));
        }
    }

    pub fn preview_off(&mut self) {
        if let Some((slot, key)) = self.preview.take() {
            self.session.send(EngineCmd::NoteOff { slot, key });
        }
    }

    pub fn fix_selection(&mut self) {
        let ids: Vec<ChannelId> = self.project.channels.iter().map(|c| c.id).collect();
        self.selected_channels.retain(|c| ids.contains(c));
        if self.selected_channel.is_none_or(|c| !ids.contains(&c)) {
            self.selected_channel = ids.first().copied();
        }
        if let Some(c) = self.selected_channel
            && self.selected_channels.is_empty()
        {
            self.selected_channels.insert(c);
        }
        self.win.instruments.retain(|c| ids.contains(c));
        self.selected_track = self.selected_track.min(self.project.mixer.len() - 1);
    }

    pub fn select_channel(&mut self, id: ChannelId, additive: bool) {
        if !additive {
            self.selected_channels.clear();
        }
        self.selected_channels.insert(id);
        self.selected_channel = Some(id);
    }

    pub fn play_stop(&mut self) {
        if self.shared.playing.load(Ordering::Relaxed) {
            self.session.send(EngineCmd::Stop);
        } else {
            self.session.send(EngineCmd::Play);
        }
    }

    // ----- keyboard ------------------------------------------------------

    fn handle_raw_keys(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        let text_input = ctx.text_edit_focused();
        let typing = self.typing_piano && !text_input;
        let mut piano_events: Vec<(soundesti_shortcuts::Key, bool)> = Vec::new();
        let keymap = &self.keymap;
        let pending = &mut self.pending;
        let focus = self.focus;
        raw.events.retain(|ev| {
            let egui::Event::Key { key, pressed, repeat, modifiers, .. } = ev else { return true };
            let Some(chord) = chord_from_egui(*key, *modifiers) else { return true };
            if typing && !chord.ctrl && !chord.alt && typing_piano_offset(chord.key).is_some() {
                if !*repeat {
                    piano_events.push((chord.key, *pressed));
                }
                return false;
            }
            if !*pressed {
                return true;
            }
            let state = InputState { context: focus, chord, text_input, typing_piano: typing };
            match keymap.resolve(state) {
                Some(b) => {
                    if !*repeat || repeatable(b.command) {
                        pending.push((b.command, b.arg));
                    }
                    false
                }
                None => true,
            }
        });
        for (key, down) in piano_events {
            self.typing_note(key, down);
        }
    }

    fn typing_note(&mut self, key: soundesti_shortcuts::Key, down: bool) {
        if down {
            let Some(off) = typing_piano_offset(key) else { return };
            let note = (self.typing_octave * 12 + off).clamp(0, 127) as u8;
            self.typing_held.insert(key, (0, note));
            self.live_note_on(note, soundesti_project::DEFAULT_VELOCITY);
        } else if let Some((_, note)) = self.typing_held.remove(&key) {
            self.live_note_off(note);
        }
    }

    // ----- command dispatch ---------------------------------------------

    pub fn run(&mut self, ctx: &egui::Context, cmd: Command, arg: Option<i32>) {
        if !self.execute(ctx, cmd, arg) {
            let keys = self.keymap.chord_for(cmd, self.focus).map(|c| format!(" ({c})")).unwrap_or_default();
            self.set_status(format!("“{}”{keys} is not implemented yet.", cmd.label()));
        }
    }

    /// Returns false if the command is not implemented (yet) in this context.
    fn execute(&mut self, ctx: &egui::Context, cmd: Command, arg: Option<i32>) -> bool {
        use Command::*;
        match cmd {
            // File
            NewProject => self.request(AfterConfirm::New),
            Open => self.request(AfterConfirm::Open(None)),
            OpenRecent => {
                let i = arg.unwrap_or(0) as usize;
                match self.recent.get(i).cloned() {
                    Some(p) => self.request(AfterConfirm::Open(Some(p))),
                    None => self.set_status(format!("No recent file #{i}.")),
                }
            }
            Save => self.save(false),
            SaveAs => self.save(true),
            SaveNewVersion => self.save_new_version(),
            ExportWav => self.start_export(),
            ExportMp3 => self.set_status(
                "MP3 export is not available yet: encoder licensing review pending (docs/LICENSE_AUDIT.md).",
            ),

            // Undo
            UndoToggle => self.undo_toggle(),
            UndoStep => {
                match self.history.undo(&mut self.project) {
                    Some(l) => self.set_status(format!("Undo: {l}")),
                    None => self.set_status("Nothing to undo."),
                }
                self.after_history();
            }
            Redo => {
                match self.history.redo(&mut self.project) {
                    Some(l) => self.set_status(format!("Redo: {l}")),
                    None => self.set_status("Nothing to redo."),
                }
                self.after_history();
            }

            // Transport
            PlayStop => self.play_stop(),
            PlayPause => {
                if self.shared.playing.load(Ordering::Relaxed) {
                    self.session.send(EngineCmd::Pause);
                } else {
                    self.session.send(EngineCmd::Play);
                }
            }
            ToggleSongMode => {
                self.project.song_mode = !self.project.song_mode;
                self.needs_sync = true;
                self.set_status(if self.project.song_mode {
                    "Song mode: playing the Playlist"
                } else {
                    "Pattern mode: looping the current pattern"
                });
            }
            ToggleRecord => self.toggle_record(),
            ToggleMetronome => {
                self.checkpoint("Metronome");
                self.project.metronome = !self.project.metronome;
                self.set_status(if self.project.metronome { "Metronome on" } else { "Metronome off" });
            }
            Panic => {
                self.session.send(EngineCmd::Panic);
                self.set_status("All sound stopped (panic).");
            }
            PlayheadToStart => {
                self.session.send(EngineCmd::Seek(0));
            }
            ToggleTypingKeyboard => {
                self.typing_piano = !self.typing_piano;
                self.set_status(if self.typing_piano {
                    "Typing keyboard to piano: ON (single-key shortcuts disabled)"
                } else {
                    "Typing keyboard to piano: OFF"
                });
            }
            TypingOctave => self.typing_octave = arg.unwrap_or(5).clamp(0, 9),
            ToggleSnapNone | ToggleGlobalSnap => {
                if self.snap == 0 {
                    self.snap = self.snap_before_none.max(1);
                } else {
                    self.snap_before_none = self.snap;
                    self.snap = 0;
                }
                let label = SNAPS.iter().find(|s| s.1 == self.snap).map(|s| s.0).unwrap_or("custom");
                self.set_status(format!("Snap: {label}"));
            }

            // Windows
            ToggleChannelRack => toggle_focus(&mut self.win.channel_rack, &mut self.focus, Context::ChannelRack),
            TogglePianoRoll => toggle_focus(&mut self.win.piano_roll, &mut self.focus, Context::PianoRoll),
            TogglePlaylist => toggle_focus(&mut self.win.playlist, &mut self.focus, Context::Playlist),
            ToggleMixer => toggle_focus(&mut self.win.mixer, &mut self.focus, Context::Mixer),
            ToggleBrowser => toggle_focus(&mut self.win.browser, &mut self.focus, Context::Browser),
            PluginPicker => self.win.plugin_picker = !self.win.plugin_picker,
            ToggleSongInfo => self.win.song_info = !self.win.song_info,
            ToggleMidiSettings => self.win.midi_settings = !self.win.midi_settings,
            Help => self.win.help = !self.win.help,
            AiSetup => self.win.ai_setup = !self.win.ai_setup,
            CloseAllWindows => {
                let w = &mut self.win;
                w.channel_rack = false;
                w.piano_roll = false;
                w.mixer = false;
                w.playlist = false;
                w.browser = false;
                w.plugin_picker = false;
                w.song_info = false;
                w.midi_settings = false;
                w.audio_settings = false;
                w.help = false;
                w.quantize = false;
                w.instruments.clear();
                w.effects.clear();
            }
            CloseWindow => {
                if self.piano.cancel_drag(&mut self.project) || self.playlist.cancel_drag(&mut self.project) {
                    self.needs_sync = true;
                    self.set_status("Edit cancelled.");
                } else {
                    self.close_focused();
                }
            }
            CycleWindows => self.cycle_windows(ctx),

            // Patterns
            SelectPattern => {
                let n = arg.unwrap_or(1) as usize;
                if let Some(p) = self.project.patterns.get(n.saturating_sub(1)) {
                    self.project.current_pattern = p.id;
                    self.needs_sync = true;
                }
            }
            NextPattern | PreviousPattern => {
                let idx = self.project.patterns.iter().position(|p| p.id == self.project.current_pattern).unwrap_or(0);
                let len = self.project.patterns.len();
                let next = if cmd == NextPattern { (idx + 1) % len } else { (idx + len - 1) % len };
                self.project.current_pattern = self.project.patterns[next].id;
                self.needs_sync = true;
            }
            NextEmptyPattern | NextEmptyPatternNamed => {
                self.checkpoint("Next empty pattern");
                let id = self.project.next_empty_pattern();
                self.project.current_pattern = id;
                if cmd == NextEmptyPatternNamed {
                    let text = self.project.current().name.clone();
                    self.dialog = Some(Dialog::RenamePattern { id, text });
                }
            }
            RenamePattern => {
                let p = self.project.current();
                self.dialog = Some(Dialog::RenamePattern { id: p.id, text: p.name.clone() });
            }

            // Tools (focused editor)
            ToolDraw | ToolPaint | ToolDelete | ToolMute | ToolSlice | ToolSelect | ToolZoom => {
                if self.focus == Context::Playlist {
                    return self.playlist_command(cmd);
                }
                if self.focus != Context::PianoRoll {
                    return false;
                }
                self.piano.tool = match cmd {
                    ToolDraw => Tool::Draw,
                    ToolPaint => Tool::Paint,
                    ToolDelete => Tool::Delete,
                    ToolMute => Tool::Mute,
                    ToolSlice => Tool::Slice,
                    ToolSelect => Tool::Select,
                    _ => Tool::Zoom,
                };
            }

            _ => {
                return match self.focus {
                    Context::PianoRoll => self.piano_command(cmd),
                    Context::ChannelRack => self.rack_command(cmd, arg),
                    Context::Mixer => self.mixer_command(cmd),
                    Context::Playlist => self.playlist_command(cmd),
                    Context::Browser => self.browser_command(cmd),
                    _ => false,
                };
            }
        }
        true
    }

    fn undo_toggle(&mut self) {
        let was_undo = self.history.can_redo();
        match self.history.toggle(&mut self.project) {
            Some(l) => self.set_status(format!("{} {l}", if was_undo { "Redo/undo:" } else { "Undo:" })),
            None => self.set_status("Nothing to undo."),
        }
        self.after_history();
    }

    fn after_history(&mut self) {
        self.revision += 1;
        self.needs_sync = true;
        self.fix_selection();
    }

    fn close_focused(&mut self) {
        let w = &mut self.win;
        match self.focus {
            Context::ChannelRack => w.channel_rack = false,
            Context::PianoRoll => w.piano_roll = false,
            Context::Playlist => w.playlist = false,
            Context::Mixer => w.mixer = false,
            Context::Browser => w.browser = false,
            Context::Global => {}
        }
    }

    fn cycle_windows(&mut self, ctx: &egui::Context) {
        let order = [
            (Context::ChannelRack, self.win.channel_rack),
            (Context::PianoRoll, self.win.piano_roll),
            (Context::Mixer, self.win.mixer),
            (Context::Playlist, self.win.playlist),
            (Context::Browser, self.win.browser),
        ];
        let open: Vec<Context> = order.iter().filter(|(_, o)| *o).map(|(c, _)| *c).collect();
        if open.is_empty() {
            return;
        }
        let i = open.iter().position(|c| *c == self.focus).map(|i| (i + 1) % open.len()).unwrap_or(0);
        self.focus = open[i];
        if let Some((layer, _)) = self.window_layers.iter().find(|(_, c)| **c == self.focus) {
            ctx.move_to_top(*layer);
        }
    }

    /// Remembers which window a layer belongs to so clicks can move keyboard focus.
    pub fn register_window(&mut self, resp: &Option<egui::InnerResponse<Option<()>>>, ctx_kind: Context) {
        if let Some(r) = resp {
            self.window_layers.insert(r.response.layer_id, ctx_kind);
        }
    }

    fn track_focus(&mut self, ctx: &egui::Context) {
        let pressed = ctx.input(|i| i.pointer.any_pressed().then(|| i.pointer.interact_pos()).flatten());
        let Some(pos) = pressed else { return };
        if let Some(c) = ctx.layer_id_at(pos).and_then(|layer| self.window_layers.get(&layer)) {
            self.focus = *c;
        } else if let Some((_, c)) = self.docked.iter().find(|(r, _)| r.contains(pos)) {
            self.focus = *c;
        }
    }

    // ----- frame ---------------------------------------------------------

    fn poll(&mut self) {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        for (i, m) in self.track_meters.iter_mut().enumerate() {
            m.update(self.shared.track_meters[i].take(), dt);
        }
        for (i, m) in self.channel_meters.iter_mut().enumerate().take(self.shared.channel_meters.len()) {
            m.update(self.shared.channel_meters[i].take(), dt);
        }
        let peak = self.shared.take_peak_cpu_load();
        self.peak_cpu = (self.peak_cpu * 0.95).max(peak);
        if self.output.health.failed.swap(false, Ordering::Relaxed) {
            self.set_status(
                "Audio device stopped (unplugged or changed). Open Options › Audio settings to restart it.",
            );
        }
        self.poll_export();
        self.poll_updates();
        self.autosave_tick();
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("SOUNDESTI").strong().color(theme::ACCENT));
            ui.separator();
            // Pattern / song mode (L).
            let song = self.project.song_mode;
            if icons::text_button(ui, icons::Icon::Pattern, "PAT", !song).on_hover_text("Pattern mode: loop the current pattern (L)").clicked() && song {
                self.run(&ui.ctx().clone(), Command::ToggleSongMode, None);
            }
            if icons::text_button(ui, icons::Icon::Song, "SONG", song).on_hover_text("Song mode: play the Playlist arrangement (L)").clicked() && !song {
                self.run(&ui.ctx().clone(), Command::ToggleSongMode, None);
            }
            let playing = self.shared.playing.load(Ordering::Relaxed);
            let icon = if playing { icons::Icon::Pause } else { icons::Icon::Play };
            if icons::button(ui, icon, playing, "Play / stop (Space)").clicked() {
                self.play_stop();
            }
            if icons::button(ui, icons::Icon::Stop, false, "Stop").clicked() {
                self.session.send(EngineCmd::Stop);
            }
            if icons::button(ui, icons::Icon::Record, self.live.armed, "Record (R): notes you play while playing; audio too if enabled in MIDI settings (F10)").clicked() {
                self.toggle_record();
            }
            ui.separator();

            let mut bpm = self.project.bpm;
            let r = ui.add(
                egui::DragValue::new(&mut bpm).range(10.0..=522.0).speed(0.2).fixed_decimals(3).suffix(" BPM"),
            );
            if r.drag_started() || r.gained_focus() {
                self.checkpoint("Tempo");
            }
            if r.changed() {
                self.project.bpm = bpm;
                self.needs_sync = true;
            }
            let tick = self.shared.position_tick().max(0.0) as Tick;
            let bst = BarStepTick::from_tick(tick, self.project.time_signature);
            ui.label(RichText::new(bst.to_string()).monospace().size(18.0).color(theme::ACCENT))
                .on_hover_text("Song position (B:S:T = bars:steps:ticks, FL Studio format)");
            // FL's toolbar master volume (same control as the Mixer master fader).
            let mut mv = self.project.mixer[0].volume;
            let tip = format!("Master volume {:+.1} dB", 20.0 * mv.max(1e-6).log10());
            let r = crate::widgets::knob(ui, &mut mv, 0.0, 1.25, 0.8, 22.0, theme::ACCENT, &tip);
            if r.started {
                self.checkpoint("Master volume");
            }
            if r.changed {
                self.project.mixer[0].volume = mv;
                self.needs_sync = true;
            }
            if icons::button(ui, icons::Icon::Metronome, self.project.metronome, "Metronome (Ctrl+M)").clicked() {
                self.run(&ui.ctx().clone(), Command::ToggleMetronome, None);
            }
            if icons::button(ui, icons::Icon::Keyboard, self.typing_piano, "Typing keyboard to piano (Ctrl+T)").clicked() {
                self.run(&ui.ctx().clone(), Command::ToggleTypingKeyboard, None);
            }
            ui.separator();

            // Pattern selector.
            let current = self.project.current().name.clone();
            egui::ComboBox::from_id_salt("pattern_sel").selected_text(current).width(110.0).show_ui(ui, |ui| {
                let ids: Vec<_> = self.project.patterns.iter().map(|p| (p.id, p.name.clone())).collect();
                for (id, name) in ids {
                    if ui.selectable_label(self.project.current_pattern == id, name).clicked() {
                        self.project.current_pattern = id;
                        self.needs_sync = true;
                    }
                }
                ui.separator();
                if ui.button("New pattern (F4)").clicked() {
                    self.run(&ui.ctx().clone(), Command::NextEmptyPatternNamed, None);
                }
            });
            // Snap.
            let snap_label = SNAPS.iter().find(|s| s.1 == self.snap).map(|s| s.0).unwrap_or("custom");
            icons::inline(ui, icons::Icon::Magnet, theme::TEXT_DIM);
            egui::ComboBox::from_id_salt("snap").selected_text(snap_label).width(84.0).show_ui(ui, |ui| {
                for (name, t) in SNAPS {
                    ui.selectable_value(&mut self.snap, *t, *name);
                }
            });
            ui.separator();

            // Window buttons, as in FL's toolbar (F5 Playlist, F6 Channel rack, F7 Piano roll,
            // F9 Mixer, Alt+F8 Browser).
            let wins = [
                (icons::Icon::Song, self.win.playlist, Command::TogglePlaylist, "Playlist (F5)"),
                (icons::Icon::Rack, self.win.channel_rack, Command::ToggleChannelRack, "Channel rack (F6)"),
                (icons::Icon::Piano, self.win.piano_roll, Command::TogglePianoRoll, "Piano roll (F7)"),
                (icons::Icon::Mixer, self.win.mixer, Command::ToggleMixer, "Mixer (F9)"),
                (icons::Icon::Folder, self.win.browser, Command::ToggleBrowser, "Browser (Alt+F8)"),
            ];
            for (icon, open, cmd, tip) in wins {
                if icons::button(ui, icon, open, tip).clicked() {
                    self.run(&ui.ctx().clone(), cmd, None);
                }
            }
            ui.separator();

            // Performance: real measurements from the audio thread.
            let load = self.shared.cpu_load();
            let overloads = self.shared.overloads.load(Ordering::Relaxed);
            let col = if self.peak_cpu > 0.9 { theme::DANGER } else if self.peak_cpu > 0.6 { theme::WARN } else { theme::TEXT_DIM };
            ui.label(RichText::new(format!("CPU {:>4.1}% (peak {:>4.1}%)", load * 100.0, self.peak_cpu * 100.0)).monospace().color(col))
                .on_hover_text(format!(
                    "Audio callback time as a share of the buffer period.\nOverloaded callbacks: {overloads}\nDevice: {} @ {} Hz",
                    self.output.info.device, self.output.info.sample_rate
                ));
            let m = self.track_meters[0];
            let (rect, _) = ui.allocate_exact_size(egui::vec2(70.0, 14.0), egui::Sense::hover());
            horizontal_meter(ui, rect, m.left, m.right);
            if self.output.info.is_null {
                icons::inline(ui, icons::Icon::Warning, theme::WARN);
                ui.label(RichText::new("NO AUDIO").color(theme::WARN)).on_hover_text("No audio device could be opened. Options › Audio settings.");
            }
        });
    }

    fn menu_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("FILE", |ui| {
                let items = [
                    (Command::NewProject, "New"),
                    (Command::Open, "Open…"),
                    (Command::Save, "Save"),
                    (Command::SaveAs, "Save as…"),
                    (Command::SaveNewVersion, "Save new version"),
                ];
                for (c, label) in items {
                    if self.menu_item(ui, c, label) {
                        self.run(&ctx, c, None);
                    }
                }
                if !self.recent.is_empty() {
                    ui.menu_button("Open recent", |ui| {
                        for (i, p) in self.recent.clone().iter().enumerate() {
                            let label = format!("{i}  {}", p.display());
                            if ui.button(label).clicked() {
                                self.request(AfterConfirm::Open(Some(p.clone())));
                                ui.close();
                            }
                        }
                    });
                }
                if ui.button("Import FL Studio project (.flp)…").on_hover_text("Brings in tempo, channels, patterns, notes and the Playlist; FL plugins become the closest Soundesti instruments").clicked() {
                    self.request(AfterConfirm::ImportFlp);
                    ui.close();
                }
                if ui.button("Open last autosave").clicked() {
                    if let Some(p) = crate::files::autosave_path().filter(|p| p.exists()) {
                        self.request(AfterConfirm::Open(Some(p)));
                    } else {
                        self.set_status("No autosave yet.");
                    }
                    ui.close();
                }
                ui.separator();
                ui.menu_button("Export", |ui| {
                    for (c, label) in [
                        (Command::ExportWav, "Wave file…"),
                        (Command::ExportMp3, "MP3 file…"),
                        (Command::ExportMidi, "MIDI file…"),
                    ] {
                        if self.menu_item(ui, c, label) {
                            self.run(&ctx, c, None);
                        }
                    }
                });
                ui.separator();
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("EDIT", |ui| {
                for (c, label) in [
                    (Command::UndoToggle, "Undo / redo last"),
                    (Command::UndoStep, "Undo step"),
                    (Command::Redo, "Redo"),
                ] {
                    if self.menu_item(ui, c, label) {
                        self.run(&ctx, c, None);
                    }
                }
            });
            ui.menu_button("ADD", |ui| {
                self.instrument_menu(ui);
            });
            ui.menu_button("PATTERNS", |ui| {
                for (c, label) in [
                    (Command::NextEmptyPatternNamed, "Find first empty…"),
                    (Command::RenamePattern, "Rename…"),
                    (Command::NextPattern, "Next pattern"),
                    (Command::PreviousPattern, "Previous pattern"),
                ] {
                    if self.menu_item(ui, c, label) {
                        self.run(&ctx, c, None);
                    }
                }
            });
            ui.menu_button("VIEW", |ui| {
                for (c, label) in [
                    (Command::TogglePlaylist, "Playlist"),
                    (Command::ToggleChannelRack, "Channel rack"),
                    (Command::TogglePianoRoll, "Piano roll"),
                    (Command::ToggleMixer, "Mixer"),
                    (Command::ToggleBrowser, "Browser"),
                    (Command::PluginPicker, "Plugin picker"),
                    (Command::CloseAllWindows, "Close all"),
                ] {
                    if self.menu_item(ui, c, label) {
                        self.run(&ctx, c, None);
                    }
                }
            });
            ui.menu_button("OPTIONS", |ui| {
                if ui.button("Manage plugins…").on_hover_text("Find VST, VST3 and CLAP plugins on this PC").clicked() {
                    self.win.plugin_manager = true;
                    ui.close();
                }
                ui.separator();
                let mut ai = self.mcp_enabled;
                if ui
                    .checkbox(&mut ai, "AI control (MCP server)")
                    .on_hover_text(format!(
                        "Lets AI assistants (Claude, etc.) control Soundesti through MCP at http://127.0.0.1:{}/mcp. Local only.",
                        crate::mcp::DEFAULT_PORT
                    ))
                    .changed()
                {
                    self.set_mcp(&ctx, ai);
                    if ai {
                        self.win.ai_setup = true;
                    }
                }
                if ui.button("Set up AI assistants…").clicked() {
                    self.win.ai_setup = true;
                    ui.close();
                }
                if ui.button("Audio settings…").clicked() {
                    self.device_names =
                        soundesti_audio_device::list_output_devices().into_iter().map(|d| d.name).collect();
                    self.win.audio_settings = true;
                    ui.close();
                }
                if self.menu_item(ui, Command::ToggleMidiSettings, "MIDI settings…") {
                    self.run(&ctx, Command::ToggleMidiSettings, None);
                }
                if self.menu_item(ui, Command::ToggleSongInfo, "Project info…") {
                    self.run(&ctx, Command::ToggleSongInfo, None);
                }
            });
            ui.menu_button("TOOLS", |ui| {
                // FL's TOOLS menu; only tools that exist are listed (see FL_COMPATIBILITY_MATRIX.md).
                if ui.button("Quantizer…").clicked() {
                    self.win.quantize = true;
                    ui.close();
                }
                for (c, label) in [
                    (Command::ToggleTypingKeyboard, "Typing keyboard to piano"),
                    (Command::Panic, "Stop all sound (panic)"),
                    (Command::ExportWav, "Render pattern to WAV…"),
                ] {
                    if self.menu_item(ui, c, label) {
                        self.run(&ctx, c, None);
                    }
                }
            });
            ui.menu_button("HELP", |ui| {
                if self.menu_item(ui, Command::Help, "Keyboard shortcuts…") {
                    self.run(&ctx, Command::Help, None);
                }
                if ui.button("AI assistant setup (Claude, Codex, Cursor…)").clicked() {
                    self.win.ai_setup = true;
                    ui.close();
                }
                ui.separator();
                self.update_menu(ui);
                ui.separator();
                if ui.button("About Soundesti").clicked() {
                    self.dialog = Some(Dialog::Message {
                        title: "About Soundesti".into(),
                        text: format!(
                            "Soundesti {} — a free, open, Rust-native DAW with an FL Studio-style workflow.\n\
                             Independent project; not affiliated with Image-Line. No telemetry.",
                            env!("CARGO_PKG_VERSION")
                        ),
                    });
                    ui.close();
                }
            });
        });
    }

    /// A menu button showing the command's shortcut; returns true if clicked.
    fn menu_item(&self, ui: &mut Ui, cmd: Command, label: &str) -> bool {
        let keys = self.keymap.chord_for(cmd, self.focus).map(|c| c.to_string()).unwrap_or_default();
        let clicked = ui.add(egui::Button::new(label).shortcut_text(keys)).clicked();
        if clicked {
            ui.close();
        }
        clicked
    }

    pub fn add_channel(&mut self, type_id: &str, name: &str) {
        self.checkpoint("Add channel");
        let id = self.project.add_channel(name, type_id);
        self.select_channel(id, false);
        self.set_status(format!("Added {name}."));
        // A new Sampler is useless without audio: ask for a file straight away.
        if type_id == soundesti_synthesis::sampler::TYPE_ID {
            self.choose_sample(id);
        }
    }

    /// Instrument list grouped by category (ADD menu and the Channel Rack "+").
    pub fn instrument_menu(&mut self, ui: &mut Ui) {
        let mut last = "";
        for info in soundesti_synthesis::CATALOG {
            if info.category != last {
                if !last.is_empty() {
                    ui.separator();
                }
                last = info.category;
                ui.label(RichText::new(info.category).small().color(theme::TEXT_DIM));
            }
            if ui.button(info.name).on_hover_text(info.description).clicked() {
                self.add_channel(info.type_id, info.name);
                ui.close();
            }
        }
        self.plugin_instrument_menu(ui);
    }

    /// Opens a file dialog and assigns a WAV file to a Sampler channel.
    pub fn choose_sample(&mut self, id: ChannelId) {
        let Some(path) =
            rfd::FileDialog::new().add_filter("WAV audio", &["wav"]).set_title("Choose a sample").pick_file()
        else {
            return;
        };
        self.set_channel_sample(id, &path);
    }

    /// Points a channel at an audio file (switching it to a Sampler if needed).
    pub fn set_channel_sample(&mut self, id: ChannelId, path: &std::path::Path) {
        if !soundesti_session::samples::is_supported(path) {
            self.set_status(format!("{} is not a WAV file.", path.display()));
            return;
        }
        if let Err(e) = soundesti_session::samples::load_wav(path) {
            self.set_status(format!("Could not load sample: {e}"));
            return;
        }
        self.checkpoint("Load sample");
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if let Some(c) = self.project.channel_mut(id) {
            if c.instrument.type_id != soundesti_synthesis::sampler::TYPE_ID {
                c.instrument.type_id = soundesti_synthesis::sampler::TYPE_ID.into();
                c.instrument.params.clear();
            }
            c.instrument.sample = Some(path.display().to_string());
            c.name = stem.clone();
        }
        self.set_status(format!("Loaded sample {stem}."));
        self.needs_sync = true;
    }

    fn hint_bar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let focus = match self.focus {
                Context::Global => "—",
                Context::ChannelRack => "Channel rack",
                Context::PianoRoll => "Piano roll",
                Context::Playlist => "Playlist",
                Context::Mixer => "Mixer",
                Context::Browser => "Browser",
            };
            icons::inline(ui, icons::Icon::Keyboard, theme::ACCENT);
            ui.label(RichText::new(focus).color(theme::ACCENT).monospace())
                .on_hover_text("Keyboard focus: shortcuts for this window take priority");
            if let Some(srv) = &self.mcp {
                ui.label(RichText::new(format!("AI :{}", srv.port)).color(theme::ACCENT).monospace())
                    .on_hover_text(format!("MCP server running at http://127.0.0.1:{}/mcp", srv.port));
            }
            ui.separator();
            ui.label(RichText::new(&self.status).color(theme::TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let info = &self.output.info;
                let lat = info.buffer_latency_ms().map(|l| format!(" · {l:.1} ms")).unwrap_or_default();
                ui.label(
                    RichText::new(format!("{} · {} Hz{lat}", info.device, info.sample_rate))
                        .color(theme::TEXT_DIM)
                        .small(),
                );
                if self.dirty() {
                    ui.label(RichText::new("unsaved").color(theme::WARN).small());
                    icons::inline(ui, icons::Icon::Dot, theme::WARN);
                }
            });
        });
    }

    fn handle_close_request(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty() && !self.quit_confirmed {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.dialog = Some(Dialog::ConfirmDiscard { then: AfterConfirm::Quit });
        }
    }

    pub fn update_title(&self, ctx: &egui::Context) {
        let name = self
            .path
            .as_ref()
            .and_then(|p| p.file_stem())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into());
        let star = if self.dirty() { "*" } else { "" };
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!("{name}{star} - Soundesti")));
    }
}

fn toggle_focus(open: &mut bool, focus: &mut Context, ctx: Context) {
    // FL behaviour: the shortcut shows the window if hidden or unfocused, hides it if focused.
    if *open && *focus == ctx {
        *open = false;
    } else {
        *open = true;
        *focus = ctx;
    }
}

/// Commands that make sense to auto-repeat while a key is held.
fn repeatable(cmd: Command) -> bool {
    use Command::*;
    matches!(
        cmd,
        MoveSelectionLeft
            | MoveSelectionRight
            | MoveSelectionUp
            | MoveSelectionDown
            | NudgeLeft
            | NudgeRight
            | NudgeUp
            | NudgeDown
            | ChannelSelectNext
            | ChannelSelectPrevious
            | ZoomIn
            | ZoomOut
            | UndoStep
            | Redo
    )
}

pub fn horizontal_meter(ui: &Ui, rect: egui::Rect, l: f32, r: f32) {
    let p = ui.painter();
    p.rect_filled(rect, 1.0, Color32::from_gray(18));
    let h = (rect.height() - 1.0) / 2.0;
    for (i, lvl) in [l, r].into_iter().enumerate() {
        let db = if lvl > 0.0 { 20.0 * lvl.log10() } else { -100.0 };
        let t = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        let y0 = rect.top() + i as f32 * (h + 1.0);
        let col = if db > 0.0 {
            theme::METER_HIGH
        } else if db > -6.0 {
            theme::METER_MID
        } else {
            theme::METER_LOW
        };
        p.rect_filled(
            egui::Rect::from_min_size(egui::pos2(rect.left(), y0), egui::vec2(rect.width() * t, h)),
            0.0,
            col,
        );
    }
}

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        self.handle_raw_keys(ctx, raw_input);
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.started {
            // Start maximized like FL; restored geometry can exceed the work area on high-DPI screens.
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
            self.started = true;
        }
        self.track_focus(&ctx);
        self.process_mcp(&ctx);
        for (cmd, arg) in std::mem::take(&mut self.pending) {
            self.run(&ctx, cmd, arg);
        }
        self.poll();
        self.handle_close_request(&ctx);

        egui::Panel::top("menu").frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(4)).show(ui, |ui| {
            self.menu_bar(ui);
            self.toolbar(ui);
        });
        egui::Panel::bottom("hint").frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(4)).show(ui, |ui| {
            self.hint_bar(ui);
        });
        // FL's default screen: Browser docked on the left, Playlist filling the workspace,
        // Channel Rack / Piano roll / Mixer floating on top.
        self.docked.clear();
        if self.win.browser {
            let r = egui::Panel::left("browser_dock")
                .resizable(true)
                .default_size(270.0)
                .size_range(180.0..=520.0)
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(0))
                .show(ui, |ui| {
                    if dock_header(ui, icons::Icon::Folder, "Browser", self.focus == Context::Browser) {
                        self.win.browser = false;
                    }
                    egui::Frame::new().inner_margin(6).show(ui, |ui| self.browser_ui(ui));
                });
            self.docked.push((r.response.rect, Context::Browser));
        }
        let r = egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG)).show(ui, |ui| {
            if self.win.playlist {
                let title = format!("Playlist - Arrangement  ›  {}", self.project.current().name);
                if dock_header(ui, icons::Icon::Song, &title, self.focus == Context::Playlist) {
                    self.win.playlist = false;
                }
                egui::Frame::new().inner_margin(4).show(ui, |ui| self.playlist_ui(ui));
            } else {
                let r = ui.max_rect();
                ui.painter().text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    "SOUNDESTI",
                    egui::FontId::proportional(64.0),
                    Color32::from_rgba_unmultiplied(255, 255, 255, 8),
                );
            }
        });
        self.workspace = r.response.rect;
        if self.win.playlist {
            self.docked.push((r.response.rect, Context::Playlist));
        }

        self.windows(&ctx);
        self.dialogs(&ctx);
        if let Some(after) = self.after.take() {
            self.perform(&ctx, after);
        }

        if self.needs_sync {
            self.session.mixer_solo = self.mixer_solo;
            self.session.sync(&self.project);
            self.needs_sync = false;
            if let Some(key) = self.pending_editor.take() {
                self.open_plugin_editor(key);
            }
        } else {
            self.session.handle().collect_garbage();
        }
        self.tick_plugins();
        self.tick_live();
        self.update_title(&ctx);

        let playing = self.shared.playing.load(Ordering::Relaxed);
        // Plugin editors need regular idle calls; scans report progress.
        let busy = playing || self.plugins.any_editor_open() || self.plugins.scanning();
        ctx.request_repaint_after(std::time::Duration::from_millis(if busy { 16 } else { 50 }));
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let recent: Vec<String> = self.recent.iter().map(|p| p.display().to_string()).collect();
        storage.set_string(RECENT_KEY, serde_json::to_string(&recent).unwrap_or_default());
        let a = &self.audio_settings;
        storage.set_string(
            AUDIO_KEY,
            serde_json::to_string(&(&a.device, a.sample_rate, a.buffer_frames)).unwrap_or_default(),
        );
        storage.set_string(crate::updates::AUTO_CHECK_KEY, self.auto_update.to_string());
        storage.set_string(MCP_KEY, self.mcp_enabled.to_string());
        storage.set_string(crate::live::MIDI_PORT_KEY, self.live.midi_port.clone().unwrap_or_default());
    }
}

/// Title strip of a docked area (FL-style window caption). Returns true when closed.
fn dock_header(ui: &mut Ui, icon: icons::Icon, title: &str, focused: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 24.0), egui::Sense::hover());
    let fill = if focused { theme::TITLE_FOCUSED } else { theme::TITLE };
    ui.painter().rect_filled(rect, 0.0, fill);
    let icon_rect =
        egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), egui::vec2(14.0, 14.0));
    icons::paint(ui.painter(), icon_rect, icon, if focused { theme::ACCENT } else { theme::TEXT_DIM });
    ui.painter().text(
        egui::pos2(rect.left() + 28.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(13.0),
        if focused { theme::TEXT } else { theme::TEXT_DIM },
    );
    let close = egui::Rect::from_center_size(egui::pos2(rect.right() - 14.0, rect.center().y), egui::vec2(18.0, 18.0));
    let r = ui.interact(close, ui.scope_id().with(("dock_close", title)), egui::Sense::click()).on_hover_text("Close");
    let c = if r.hovered() { theme::TEXT } else { theme::TEXT_DIM };
    let k = 4.5;
    ui.painter().line_segment(
        [close.center() + egui::vec2(-k, -k), close.center() + egui::vec2(k, k)],
        egui::Stroke::new(1.5, c),
    );
    ui.painter().line_segment(
        [close.center() + egui::vec2(-k, k), close.center() + egui::vec2(k, -k)],
        egui::Stroke::new(1.5, c),
    );
    r.clicked()
}
