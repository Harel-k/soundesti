//! Context-sensitive keyboard shortcuts.
//!
//! - [`Keymap`] is a profile loaded from TOML (the bundled one is
//!   [`Keymap::fl_studio_workflow`]). Loading validates every key, context and
//!   command ID and rejects conflicting bindings.
//! - [`Keymap::resolve`] applies FL's dispatch rules: focused-window bindings win over
//!   global ones, text fields swallow keys, and Typing-keyboard-to-piano mode
//!   suppresses single-key shortcuts that are piano keys.

mod key;
mod piano;

pub use key::{Chord, Key, ParseKeyError};
pub use piano::typing_piano_offset;

use serde::{Deserialize, Serialize};
use soundesti_commands::Command;
use std::collections::HashMap;

/// Where keyboard focus is. Mirrors FL's per-window shortcut tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Context {
    Global,
    ChannelRack,
    PianoRoll,
    Playlist,
    Mixer,
    Browser,
}

impl Context {
    pub const ALL: [Context; 6] = [
        Context::Global,
        Context::ChannelRack,
        Context::PianoRoll,
        Context::Playlist,
        Context::Mixer,
        Context::Browser,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Context::Global => "global",
            Context::ChannelRack => "channel_rack",
            Context::PianoRoll => "piano_roll",
            Context::Playlist => "playlist",
            Context::Mixer => "mixer",
            Context::Browser => "browser",
        }
    }

    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == s)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub context: Context,
    pub chord: Chord,
    pub command: Command,
    pub arg: Option<i32>,
    pub section: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum KeymapError {
    #[error("invalid TOML: {0}")]
    Toml(String),
    #[error("binding {index}: unknown context `{context}`")]
    UnknownContext { index: usize, context: String },
    #[error("binding {index}: unknown command `{command}`")]
    UnknownCommand { index: usize, command: String },
    #[error("binding {index}: {source}")]
    BadKey { index: usize, source: ParseKeyError },
    #[error("`{keys}` in {context} is bound twice (bindings {first} and {second})")]
    Conflict { context: &'static str, keys: String, first: usize, second: usize },
}

#[derive(Serialize, Deserialize)]
struct RawBinding {
    ctx: String,
    keys: String,
    cmd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    arg: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    section: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    note: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct RawKeymap {
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    bindings: Vec<RawBinding>,
}

/// What the dispatcher knows about the current keyboard state.
#[derive(Clone, Copy, Debug)]
pub struct InputState {
    pub context: Context,
    pub chord: Chord,
    /// A text field has keyboard focus.
    pub text_input: bool,
    /// Typing keyboard to piano (Ctrl+T) is on.
    pub typing_piano: bool,
}

#[derive(Clone, Debug)]
pub struct Keymap {
    pub name: String,
    pub source: Option<String>,
    bindings: Vec<Binding>,
    index: HashMap<(Context, Chord), usize>,
}

pub const FL_STUDIO_WORKFLOW_TOML: &str = include_str!("../../../assets/shortcuts/fl-studio-workflow.toml");

impl Keymap {
    /// The bundled "FL Studio Workflow" profile.
    pub fn fl_studio_workflow() -> Self {
        Self::from_toml(FL_STUDIO_WORKFLOW_TOML).expect("bundled FL Studio Workflow profile must be valid")
    }

    pub fn from_toml(src: &str) -> Result<Self, KeymapError> {
        let raw: RawKeymap = toml::from_str(src).map_err(|e| KeymapError::Toml(e.to_string()))?;
        let mut bindings = Vec::with_capacity(raw.bindings.len());
        let mut index = HashMap::new();
        for (i, b) in raw.bindings.into_iter().enumerate() {
            let context = Context::from_id(&b.ctx)
                .ok_or_else(|| KeymapError::UnknownContext { index: i, context: b.ctx.clone() })?;
            let command = Command::from_id(&b.cmd)
                .ok_or_else(|| KeymapError::UnknownCommand { index: i, command: b.cmd.clone() })?;
            let chord: Chord = b.keys.parse().map_err(|source| KeymapError::BadKey { index: i, source })?;
            if let Some(&first) = index.get(&(context, chord)) {
                return Err(KeymapError::Conflict { context: context.id(), keys: b.keys, first, second: i });
            }
            index.insert((context, chord), i);
            bindings.push(Binding { context, chord, command, arg: b.arg, section: b.section, note: b.note });
        }
        Ok(Self { name: raw.name, source: raw.source, bindings, index })
    }

    /// Serializes the profile back to TOML (for shortcut-profile export).
    pub fn to_toml(&self) -> String {
        let raw = RawKeymap {
            name: self.name.clone(),
            source: self.source.clone(),
            bindings: self
                .bindings
                .iter()
                .map(|b| RawBinding {
                    ctx: b.context.id().into(),
                    keys: b.chord.to_string(),
                    cmd: b.command.id().into(),
                    arg: b.arg,
                    section: b.section.clone(),
                    note: b.note.clone(),
                })
                .collect(),
        };
        toml::to_string(&raw).expect("keymap serialization cannot fail")
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn lookup(&self, context: Context, chord: Chord) -> Option<&Binding> {
        self.index.get(&(context, chord)).map(|&i| &self.bindings[i])
    }

    /// Resolves a key press to a command following FL's context rules.
    pub fn resolve(&self, input: InputState) -> Option<&Binding> {
        let chord = input.chord;
        if input.text_input && !matches!(chord.key, Key::F(_)) {
            return None;
        }
        if input.typing_piano && !chord.ctrl && !chord.alt && typing_piano_offset(chord.key).is_some() {
            return None;
        }
        if input.context != Context::Global
            && let Some(b) = self.lookup(input.context, chord)
        {
            return Some(b);
        }
        self.lookup(Context::Global, chord)
    }

    /// First chord bound to `command` (preferring `context`), for menus and tooltips.
    pub fn chord_for(&self, command: Command, context: Context) -> Option<Chord> {
        let mut fallback = None;
        for b in &self.bindings {
            if b.command == command {
                if b.context == context {
                    return Some(b.chord);
                }
                if b.context == Context::Global || fallback.is_none() {
                    fallback = Some(b.chord);
                }
            }
        }
        fallback
    }

    /// Case-insensitive search over command labels, IDs and key names (shortcut search panel).
    pub fn search(&self, query: &str) -> Vec<&Binding> {
        let q = query.to_lowercase();
        self.bindings
            .iter()
            .filter(|b| {
                q.is_empty()
                    || b.command.label().to_lowercase().contains(&q)
                    || b.command.id().contains(&q)
                    || b.chord.to_string().to_lowercase().contains(&q)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        s.parse().unwrap()
    }

    fn press(map: &Keymap, ctx: Context, keys: &str) -> Option<Command> {
        map.resolve(InputState { context: ctx, chord: chord(keys), text_input: false, typing_piano: false })
            .map(|b| b.command)
    }

    #[test]
    fn bundled_profile_loads_without_conflicts() {
        let map = Keymap::fl_studio_workflow();
        assert_eq!(map.name, "FL Studio Workflow");
        assert!(map.bindings().len() > 250, "only {} bindings", map.bindings().len());
    }

    #[test]
    fn mandatory_prompt_mappings() {
        use Command::*;
        let m = Keymap::fl_studio_workflow();
        let g = Context::Global;
        for (keys, cmd) in [
            ("Space", PlayStop),
            ("Ctrl+S", Save),
            ("Ctrl+Shift+S", SaveAs),
            ("Ctrl+O", Open),
            ("Ctrl+N", SaveNewVersion),
            ("Ctrl+R", ExportWav),
            ("Ctrl+Shift+R", ExportMp3),
            ("Ctrl+M", ToggleMetronome),
            ("Ctrl+H", Panic),
            ("R", ToggleRecord),
            ("L", ToggleSongMode),
            ("Ctrl+T", ToggleTypingKeyboard),
            ("F1", Help),
            ("F5", TogglePlaylist),
            ("F6", ToggleChannelRack),
            ("F7", TogglePianoRoll),
            ("F8", PluginPicker),
            ("F9", ToggleMixer),
            ("F10", ToggleMidiSettings),
            ("F11", ToggleSongInfo),
            ("F12", CloseAllWindows),
            ("Alt+F8", ToggleBrowser),
            ("Esc", CloseWindow),
            ("F2", RenamePattern),
            ("F4", NextEmptyPatternNamed),
        ] {
            assert_eq!(press(&m, g, keys), Some(cmd), "global {keys}");
        }
        let pr = Context::PianoRoll;
        for (keys, cmd) in [
            ("P", ToolDraw),
            ("B", ToolPaint),
            ("C", ToolSlice),
            ("D", ToolDelete),
            ("E", ToolSelect),
            ("T", ToolMute),
            ("Z", ToolZoom),
            ("Ctrl+A", SelectAll),
            ("Ctrl+B", DuplicateRight),
            ("Ctrl+C", Copy),
            ("Ctrl+V", Paste),
            ("Ctrl+X", Cut),
            ("Ctrl+Q", PrQuickQuantize),
            ("Ctrl+L", PrQuickLegato),
            ("Ctrl+M", PrImportMidi),
            ("Alt+Q", PrQuantize),
            ("Alt+R", PrRandomize),
            ("Alt+S", PrStrum),
            ("Alt+U", PrChop),
            ("Alt+O", PrLfo),
            ("Alt+V", PrToggleGhostChannels),
            ("Ctrl+Up", TransposeOctaveUp),
            ("Ctrl+Down", TransposeOctaveDown),
        ] {
            assert_eq!(press(&m, pr, keys), Some(cmd), "piano roll {keys}");
        }
        let pl = Context::Playlist;
        for (keys, cmd) in [
            ("P", ToolDraw),
            ("B", ToolPaint),
            ("C", ToolSlice),
            ("D", ToolDelete),
            ("E", ToolSelect),
            ("S", ToolSlip),
            ("T", ToolMute),
            ("Z", ToolZoom),
            ("Ctrl+B", DuplicateRight),
            ("Ctrl+A", SelectAll),
            ("Home", PlayheadToStart),
            ("Ins", PlSliceAtCursor),
            ("Alt+T", PlAddTimeMarker),
            ("Shift+Q", PlQuickQuantizeStarts),
        ] {
            assert_eq!(press(&m, pl, keys), Some(cmd), "playlist {keys}");
        }
        assert_eq!(press(&m, Context::ChannelRack, "Ctrl+L"), Some(ChannelRouteToFreeTracks));
    }

    #[test]
    fn same_key_differs_by_context() {
        let m = Keymap::fl_studio_workflow();
        assert_eq!(press(&m, Context::Playlist, "S"), Some(Command::ToolSlip));
        assert_eq!(press(&m, Context::PianoRoll, "S"), Some(Command::PrToggleSlide));
        assert_eq!(press(&m, Context::Mixer, "S"), Some(Command::MixerSolo));
        assert_eq!(press(&m, Context::Global, "S"), None);
        // Ctrl+M: metronome globally, Import MIDI in the piano roll.
        assert_eq!(press(&m, Context::Mixer, "Ctrl+M"), Some(Command::ToggleMetronome));
        assert_eq!(press(&m, Context::PianoRoll, "Ctrl+M"), Some(Command::PrImportMidi));
        // F2: rename pattern globally, rename track in the mixer.
        assert_eq!(press(&m, Context::Mixer, "F2"), Some(Command::MixerRename));
        assert_eq!(press(&m, Context::ChannelRack, "F2"), Some(Command::RenamePattern));
    }

    #[test]
    fn context_falls_back_to_global() {
        let m = Keymap::fl_studio_workflow();
        for ctx in Context::ALL {
            assert_eq!(press(&m, ctx, "Space"), Some(Command::PlayStop), "{ctx:?}");
            assert_eq!(press(&m, ctx, "F9"), Some(Command::ToggleMixer), "{ctx:?}");
        }
    }

    #[test]
    fn text_fields_swallow_shortcuts_except_function_keys() {
        // The master prompt's regression scenario.
        let m = Keymap::fl_studio_workflow();
        let mut st =
            InputState { context: Context::Playlist, chord: chord("P"), text_input: false, typing_piano: false };
        assert_eq!(m.resolve(st).map(|b| b.command), Some(Command::ToolDraw));
        st.context = Context::PianoRoll;
        assert_eq!(m.resolve(st).map(|b| b.command), Some(Command::ToolDraw));
        st.text_input = true;
        assert!(m.resolve(st).is_none(), "P must type into the text field");
        st.chord = chord("Ctrl+S");
        assert!(m.resolve(st).is_none(), "Ctrl+S belongs to the text field while editing");
        st.chord = chord("F9");
        assert_eq!(m.resolve(st).map(|b| b.command), Some(Command::ToggleMixer));
    }

    #[test]
    fn typing_piano_suppresses_single_key_shortcuts() {
        let m = Keymap::fl_studio_workflow();
        let mut st =
            InputState { context: Context::PianoRoll, chord: chord("P"), text_input: false, typing_piano: true };
        assert!(m.resolve(st).is_none(), "P is a piano key while typing keyboard is on");
        st.chord = chord("R");
        assert!(m.resolve(st).is_none());
        st.chord = chord("Space");
        assert_eq!(m.resolve(st).map(|b| b.command), Some(Command::PlayStop));
        st.chord = chord("Ctrl+T");
        assert_eq!(m.resolve(st).map(|b| b.command), Some(Command::ToggleTypingKeyboard));
        st.typing_piano = false;
        st.chord = chord("P");
        assert_eq!(m.resolve(st).map(|b| b.command), Some(Command::ToolDraw));
    }

    #[test]
    fn conflicts_and_bad_entries_are_rejected() {
        let dup = r#"
            name = "x"
            bindings = [
              { ctx = "global", keys = "Ctrl+S", cmd = "file.save" },
              { ctx = "global", keys = "ctrl+s", cmd = "file.open" },
            ]"#;
        assert!(matches!(Keymap::from_toml(dup), Err(KeymapError::Conflict { .. })));
        let bad_cmd = r#"name = "x"
            bindings = [{ ctx = "global", keys = "A", cmd = "does.not_exist" }]"#;
        assert!(matches!(Keymap::from_toml(bad_cmd), Err(KeymapError::UnknownCommand { .. })));
        let bad_ctx = r#"name = "x"
            bindings = [{ ctx = "kitchen", keys = "A", cmd = "file.save" }]"#;
        assert!(matches!(Keymap::from_toml(bad_ctx), Err(KeymapError::UnknownContext { .. })));
        let bad_key = r#"name = "x"
            bindings = [{ ctx = "global", keys = "Ctrl+Banana", cmd = "file.save" }]"#;
        assert!(matches!(Keymap::from_toml(bad_key), Err(KeymapError::BadKey { .. })));
        // Same chord in different contexts is fine.
        let ok = r#"name = "x"
            bindings = [
              { ctx = "global", keys = "S", cmd = "file.save" },
              { ctx = "mixer", keys = "S", cmd = "mixer.solo" },
            ]"#;
        assert!(Keymap::from_toml(ok).is_ok());
    }

    #[test]
    fn export_import_round_trip() {
        let m = Keymap::fl_studio_workflow();
        let back = Keymap::from_toml(&m.to_toml()).unwrap();
        assert_eq!(back.bindings(), m.bindings());
    }

    #[test]
    fn every_bundled_binding_resolves_in_its_context() {
        // Generated regression: each matrix row must dispatch to its own command.
        let m = Keymap::fl_studio_workflow();
        for b in m.bindings() {
            let got =
                m.resolve(InputState { context: b.context, chord: b.chord, text_input: false, typing_piano: false });
            assert_eq!(got.map(|g| (g.command, g.arg)), Some((b.command, b.arg)), "{:?} {}", b.context, b.chord);
        }
    }

    #[test]
    fn search_and_chord_lookup() {
        let m = Keymap::fl_studio_workflow();
        assert!(m.search("metronome").iter().any(|b| b.command == Command::ToggleMetronome));
        assert!(m.search("ctrl+shift+s").iter().any(|b| b.command == Command::SaveAs));
        assert_eq!(m.chord_for(Command::ToggleMixer, Context::PianoRoll), Some(chord("F9")));
        assert_eq!(m.chord_for(Command::ToolDraw, Context::Playlist), Some(chord("P")));
    }
}
