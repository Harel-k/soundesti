//! MCP tool catalogue and handlers. Every tool edits the real project model through the
//! same paths the UI uses (undo checkpoints, session sync), so anything an AI does can be
//! undone with Ctrl+Z and is saved like a normal edit.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

use serde_json::{Value, json};
use soundesti_audio::Command as EngineCmd;
use soundesti_commands::Command;
use soundesti_project::{ChannelId, DEFAULT_VELOCITY, Note};
use soundesti_shortcuts::{Context, InputState};
use soundesti_timeline::{BarStepTick, PPQ, TICKS_PER_STEP, Tick};

use crate::app::{AfterConfirm, App};
use crate::mcp::{ToolOutput, ToolResult};

pub const INSTRUCTIONS: &str = "Soundesti is a DAW with an FL Studio-style workflow. Start with get_state. \
Every menu item, shortcut and editor tool is a command: list_commands shows them all and run_command executes one \
(pass `window` to target an editor, e.g. piano_roll for tool/edit commands). Direct tools edit channels, steps, notes, \
instrument parameters, mixer tracks, patterns and transport. Musical time is in ticks (960 per quarter note, 240 per \
step) unless a tool says otherwise. Every edit is undoable (run_command edit.undo_toggle). Use screenshot to see the UI.";

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {"type": "object", "properties": properties, "required": required},
    })
}

const CHANNEL: &str = "Channel id (number) or exact channel name (string).";
const WINDOWS: &[&str] = &[
    "channel_rack",
    "piano_roll",
    "mixer",
    "playlist",
    "browser",
    "plugin_picker",
    "project_info",
    "midi_settings",
    "audio_settings",
    "shortcuts",
    "quantizer",
    "export",
    "ai_setup",
];

pub fn tool_definitions() -> Vec<Value> {
    let ch = json!({"type": ["integer", "string"], "description": CHANNEL});
    vec![
        tool(
            "get_state",
            "Snapshot of the whole session: transport, tempo, patterns, channels, notes count, mixer, windows, focus, file.",
            json!({}),
            &[],
        ),
        tool(
            "list_commands",
            "Every command in Soundesti (menus, shortcuts, tools) with its id, label, category and FL Studio shortcut. Optional text filter.",
            json!({"filter": {"type": "string"}}),
            &[],
        ),
        tool(
            "run_command",
            "Run any Soundesti command by id (see list_commands), exactly as if its shortcut/menu item were used. `window` sets keyboard focus first so editor commands (tools, copy/paste, quantize…) apply to that editor.",
            json!({
                "command": {"type": "string", "description": "Command id, e.g. transport.play_stop or edit.duplicate_right"},
                "arg": {"type": "integer", "description": "Numeric argument for commands that take one (pattern number, channel number…)"},
                "window": {"type": "string", "enum": ["global", "channel_rack", "piano_roll", "playlist", "mixer", "browser"]},
            }),
            &["command"],
        ),
        tool(
            "press_shortcut",
            "Press a keyboard shortcut in a window using the FL Studio Workflow profile, e.g. {\"keys\":\"Ctrl+B\",\"window\":\"piano_roll\"}.",
            json!({"keys": {"type": "string"}, "window": {"type": "string", "enum": ["global", "channel_rack", "piano_roll", "playlist", "mixer", "browser"]}}),
            &["keys"],
        ),
        tool(
            "transport",
            "Control playback.",
            json!({"action": {"type": "string", "enum": ["play", "stop", "pause", "play_stop", "seek"]}, "tick": {"type": "integer", "description": "For seek"}}),
            &["action"],
        ),
        tool("set_tempo", "Set the project tempo in BPM (10-522).", json!({"bpm": {"type": "number"}}), &["bpm"]),
        tool(
            "add_channel",
            "Add an instrument channel to the Channel Rack. Returns its id.",
            json!({"instrument": {"type": "string", "description": "Instrument type id; default soundesti.trident"}, "name": {"type": "string"}}),
            &[],
        ),
        tool(
            "remove_channel",
            "Delete a channel and its notes in every pattern.",
            json!({"channel": ch}),
            &["channel"],
        ),
        tool(
            "set_channel",
            "Change a channel's name, volume (0-1.25 linear, default 0.78), pan (-1..1), mute, or target mixer track (0 = master, 1-125 inserts). Also selects it.",
            json!({"channel": ch, "name": {"type": "string"}, "volume": {"type": "number"}, "pan": {"type": "number"}, "muted": {"type": "boolean"}, "mixer_track": {"type": "integer"}, "select": {"type": "boolean"}}),
            &["channel"],
        ),
        tool(
            "set_steps",
            "Set a channel's step-sequencer steps in the current pattern. `steps` is a string like \"x...x...x...x...\" (x = on) or a list of step numbers (0-based). Replaces the channel's notes in this pattern.",
            json!({"channel": ch, "steps": {"type": ["string", "array"], "items": {"type": "integer"}}}),
            &["channel", "steps"],
        ),
        tool(
            "add_notes",
            "Add notes to a channel in the current pattern (Piano roll). Times in ticks (960/quarter) or, with unit=steps, in 1/16 steps. key is a MIDI note (60 = C5 in FL naming). velocity 0-1.",
            json!({
                "channel": ch,
                "unit": {"type": "string", "enum": ["ticks", "steps"]},
                "notes": {"type": "array", "items": {"type": "object", "properties": {
                    "start": {"type": "number"}, "length": {"type": "number"}, "key": {"type": "integer"}, "velocity": {"type": "number"}
                }, "required": ["start", "length", "key"]}},
            }),
            &["channel", "notes"],
        ),
        tool(
            "get_notes",
            "List a channel's notes in the current pattern (ticks).",
            json!({"channel": ch}),
            &["channel"],
        ),
        tool(
            "clear_notes",
            "Remove all of a channel's notes in the current pattern.",
            json!({"channel": ch}),
            &["channel"],
        ),
        tool(
            "get_instrument_params",
            "List a channel's instrument parameters with ranges and current values.",
            json!({"channel": ch}),
            &["channel"],
        ),
        tool(
            "set_instrument_param",
            "Set an instrument parameter by key (see get_instrument_params), e.g. filter.cutoff.",
            json!({"channel": ch, "param": {"type": "string"}, "value": {"type": "number"}}),
            &["channel", "param", "value"],
        ),
        tool(
            "set_mixer_track",
            "Change a mixer track (0 = master, 1-125 inserts): volume (0-1.25 linear, default 0.8), pan, mute, name, solo.",
            json!({"track": {"type": "integer"}, "volume": {"type": "number"}, "pan": {"type": "number"}, "muted": {"type": "boolean"}, "name": {"type": "string"}, "solo": {"type": "boolean"}}),
            &["track"],
        ),
        tool(
            "pattern",
            "Select, create or rename patterns.",
            json!({"action": {"type": "string", "enum": ["select", "new", "rename"]}, "pattern": {"type": "integer", "description": "Pattern id (select/rename)"}, "name": {"type": "string"}}),
            &["action"],
        ),
        tool(
            "set_window",
            "Open or close a window.",
            json!({"window": {"type": "string", "enum": WINDOWS}, "open": {"type": "boolean"}}),
            &["window", "open"],
        ),
        tool(
            "save_project",
            "Save the project. Without a path, saves to the current file (fails if never saved). Path must end in .soundesti.",
            json!({"path": {"type": "string"}}),
            &[],
        ),
        tool(
            "open_project",
            "Open a .soundesti project. Fails if there are unsaved changes unless discard_unsaved is true.",
            json!({"path": {"type": "string"}, "discard_unsaved": {"type": "boolean"}}),
            &["path"],
        ),
        tool(
            "new_project",
            "Start a new project. Fails if there are unsaved changes unless discard_unsaved is true.",
            json!({"discard_unsaved": {"type": "boolean"}}),
            &[],
        ),
        tool(
            "export_wav",
            "Render the current pattern to a WAV file (same engine as playback). Path must end in .wav.",
            json!({"path": {"type": "string"}, "loops": {"type": "integer"}, "tail_seconds": {"type": "number"}, "sample_rate": {"type": "integer"}, "bit_depth": {"type": "string", "enum": ["16", "24", "32f"]}}),
            &["path"],
        ),
        tool(
            "screenshot",
            "Capture the Soundesti window as a PNG image (downscaled to at most 1600 px wide).",
            json!({}),
            &[],
        ),
    ]
}

// ----- argument helpers ----------------------------------------------------

fn s<'a>(a: &'a Value, k: &str) -> Option<&'a str> {
    a.get(k).and_then(Value::as_str)
}
fn f(a: &Value, k: &str) -> Option<f64> {
    a.get(k).and_then(Value::as_f64)
}
fn i(a: &Value, k: &str) -> Option<i64> {
    a.get(k).and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|x| x as i64)))
}
fn b(a: &Value, k: &str) -> Option<bool> {
    a.get(k).and_then(Value::as_bool)
}
fn text(v: Value) -> ToolResult {
    Ok(ToolOutput::Text(serde_json::to_string_pretty(&v).unwrap_or_default()))
}
fn context_from(name: &str) -> Result<Context, String> {
    Context::from_id(name).ok_or_else(|| format!("unknown window `{name}`"))
}

impl App {
    /// Applies pending MCP tool calls. Called once per frame on the UI thread.
    pub fn process_mcp(&mut self, ctx: &egui::Context) {
        let calls: Vec<_> = match &self.mcp {
            Some(srv) => srv.calls.try_iter().collect(),
            None => return,
        };
        for call in calls {
            if call.name == "screenshot" {
                self.mcp_screenshot(ctx, call.reply);
                continue;
            }
            if call.name == "export_wav" {
                self.mcp_export(&call.args, call.reply);
                continue;
            }
            let res = self.mcp_call(ctx, &call.name, &call.args);
            let _ = call.reply.send(res);
        }
    }

    fn resolve_channel(&self, v: Option<&Value>) -> Result<ChannelId, String> {
        let v = v.ok_or("missing `channel`")?;
        let found = if let Some(n) = v.as_u64() {
            self.project.channels.iter().find(|c| c.id.0 as u64 == n)
        } else if let Some(name) = v.as_str() {
            self.project.channels.iter().find(|c| c.name == name)
        } else {
            None
        };
        found.map(|c| c.id).ok_or_else(|| format!("no channel {v}"))
    }

    fn mcp_call(&mut self, ctx: &egui::Context, name: &str, a: &Value) -> ToolResult {
        match name {
            "get_state" => text(self.state_json()),
            "list_commands" => {
                let filter = s(a, "filter").unwrap_or("").to_lowercase();
                let list: Vec<Value> = Command::ALL
                    .iter()
                    .filter(|c| filter.is_empty() || c.id().contains(&filter) || c.label().to_lowercase().contains(&filter))
                    .map(|c| {
                        let keys: Vec<Value> = self
                            .keymap
                            .bindings()
                            .iter()
                            .filter(|bnd| bnd.command == *c)
                            .map(|bnd| json!({"keys": bnd.chord.to_string(), "window": bnd.context.id(), "arg": bnd.arg}))
                            .collect();
                        json!({"id": c.id(), "label": c.label(), "category": format!("{:?}", c.category()), "shortcuts": keys})
                    })
                    .collect();
                text(json!({"count": list.len(), "commands": list}))
            }
            "run_command" => {
                let id = s(a, "command").ok_or("missing `command`")?;
                let cmd = Command::from_id(id).ok_or_else(|| format!("unknown command `{id}` (see list_commands)"))?;
                if let Some(w) = s(a, "window") {
                    self.focus = context_from(w)?;
                }
                self.status.clear();
                let arg = i(a, "arg").map(|x| x as i32);
                let implemented = self.execute_pub(ctx, cmd, arg);
                if !implemented {
                    return Err(format!("`{id}` ({}) is not implemented yet in this context", cmd.label()));
                }
                text(json!({"ok": true, "command": id, "status": self.status}))
            }
            "press_shortcut" => {
                let keys = s(a, "keys").ok_or("missing `keys`")?;
                let chord = keys.parse().map_err(|e| format!("{e}"))?;
                let context = match s(a, "window") {
                    Some(w) => context_from(w)?,
                    None => self.focus,
                };
                let binding = self
                    .keymap
                    .resolve(InputState { context, chord, text_input: false, typing_piano: false })
                    .cloned()
                    .ok_or_else(|| format!("`{keys}` does nothing in {}", context.id()))?;
                self.focus = context;
                self.status.clear();
                if !self.execute_pub(ctx, binding.command, binding.arg) {
                    return Err(format!("`{keys}` → {} is not implemented yet", binding.command.id()));
                }
                text(json!({"ok": true, "command": binding.command.id(), "status": self.status}))
            }
            "transport" => {
                let cmd = match s(a, "action").ok_or("missing `action`")? {
                    "play" => EngineCmd::Play,
                    "stop" => EngineCmd::Stop,
                    "pause" => EngineCmd::Pause,
                    "play_stop" => {
                        self.play_stop();
                        return text(json!({"ok": true}));
                    }
                    "seek" => EngineCmd::Seek(i(a, "tick").ok_or("seek needs `tick`")?),
                    other => return Err(format!("unknown action `{other}`")),
                };
                self.session.send(cmd);
                text(json!({"ok": true}))
            }
            "set_tempo" => {
                let bpm = f(a, "bpm").ok_or("missing `bpm`")?;
                if !(10.0..=522.0).contains(&bpm) {
                    return Err("bpm must be between 10 and 522".into());
                }
                self.checkpoint("Tempo (AI)");
                self.project.bpm = bpm;
                text(json!({"ok": true, "bpm": bpm}))
            }
            "add_channel" => {
                let ty = s(a, "instrument").unwrap_or(soundesti_synthesis::trident::TYPE_ID);
                let label = soundesti_synthesis::INSTRUMENT_TYPES
                    .iter()
                    .find(|(t, _)| *t == ty)
                    .map(|(_, n)| *n)
                    .ok_or_else(|| format!("unknown instrument `{ty}`; available: {:?}", soundesti_synthesis::INSTRUMENT_TYPES))?;
                self.checkpoint("Add channel (AI)");
                let id = self.project.add_channel(s(a, "name").unwrap_or(label), ty);
                self.select_channel(id, false);
                text(json!({"ok": true, "channel": id.0}))
            }
            "remove_channel" => {
                let id = self.resolve_channel(a.get("channel"))?;
                self.checkpoint("Delete channel (AI)");
                self.project.remove_channel(id);
                self.fix_selection();
                text(json!({"ok": true}))
            }
            "set_channel" => {
                let id = self.resolve_channel(a.get("channel"))?;
                self.checkpoint("Edit channel (AI)");
                let c = self.project.channel_mut(id).unwrap();
                if let Some(n) = s(a, "name") {
                    c.name = n.to_string();
                }
                if let Some(v) = f(a, "volume") {
                    c.volume = (v as f32).clamp(0.0, 1.25);
                }
                if let Some(p) = f(a, "pan") {
                    c.pan = (p as f32).clamp(-1.0, 1.0);
                }
                if let Some(m) = b(a, "muted") {
                    c.muted = m;
                }
                if let Some(t) = i(a, "mixer_track") {
                    c.mixer_track = t.clamp(0, 125) as u16;
                }
                if b(a, "select").unwrap_or(true) {
                    self.select_channel(id, false);
                }
                text(json!({"ok": true}))
            }
            "set_steps" => {
                let id = self.resolve_channel(a.get("channel"))?;
                let steps: Vec<i64> = match a.get("steps") {
                    Some(Value::String(p)) => p.chars().filter(|c| !c.is_whitespace()).enumerate().filter(|(_, c)| matches!(c, 'x' | 'X' | '1')).map(|(n, _)| n as i64).collect(),
                    Some(Value::Array(v)) => v.iter().filter_map(Value::as_i64).collect(),
                    _ => return Err("`steps` must be a string like \"x...x...\" or a list of step numbers".into()),
                };
                if steps.iter().any(|&st| !(0..1024).contains(&st)) {
                    return Err("step numbers must be 0-1023".into());
                }
                self.checkpoint("Set steps (AI)");
                let notes = self.project.current_mut().notes_mut(id);
                notes.clear();
                for st in &steps {
                    notes.push(Note::new(st * TICKS_PER_STEP, TICKS_PER_STEP, soundesti_project::DEFAULT_NOTE_KEY));
                }
                text(json!({"ok": true, "steps_on": steps}))
            }
            "add_notes" => {
                let id = self.resolve_channel(a.get("channel"))?;
                let scale = if s(a, "unit") == Some("steps") { TICKS_PER_STEP as f64 } else { 1.0 };
                let list = a.get("notes").and_then(Value::as_array).ok_or("`notes` must be an array")?;
                let mut new = Vec::with_capacity(list.len());
                for n in list {
                    let start = (f(n, "start").ok_or("note needs `start`")? * scale).round() as Tick;
                    let length = (f(n, "length").ok_or("note needs `length`")? * scale).round() as Tick;
                    let key = i(n, "key").ok_or("note needs `key`")?;
                    if start < 0 || length <= 0 || !(0..128).contains(&key) {
                        return Err(format!("invalid note {n}"));
                    }
                    let mut note = Note::new(start, length, key as u8);
                    note.velocity = f(n, "velocity").map(|v| v as f32).unwrap_or(DEFAULT_VELOCITY).clamp(0.0, 1.0);
                    new.push(note);
                }
                self.checkpoint("Add notes (AI)");
                let notes = self.project.current_mut().notes_mut(id);
                notes.extend(new.iter().copied());
                notes.sort_by_key(|n| (n.start, n.key));
                text(json!({"ok": true, "added": new.len()}))
            }
            "get_notes" => {
                let id = self.resolve_channel(a.get("channel"))?;
                let notes: Vec<Value> = self
                    .project
                    .current()
                    .notes(id)
                    .iter()
                    .map(|n| json!({"start": n.start, "length": n.length, "key": n.key, "velocity": n.velocity, "muted": n.muted}))
                    .collect();
                text(json!({"ppq": PPQ, "ticks_per_step": TICKS_PER_STEP, "notes": notes}))
            }
            "clear_notes" => {
                let id = self.resolve_channel(a.get("channel"))?;
                self.checkpoint("Clear notes (AI)");
                self.project.current_mut().notes_mut(id).clear();
                text(json!({"ok": true}))
            }
            "get_instrument_params" => {
                let id = self.resolve_channel(a.get("channel"))?;
                let ch = self.project.channel(id).unwrap();
                let params: Vec<Value> = soundesti_session::instrument_params(&ch.instrument.type_id)
                    .iter()
                    .map(|p| {
                        let cur = ch.instrument.params.get(p.key).copied().unwrap_or(p.default);
                        json!({"key": p.key, "name": p.name, "group": p.group, "min": p.min, "max": p.max, "default": p.default, "value": cur, "unit": p.unit})
                    })
                    .collect();
                text(json!({"instrument": ch.instrument.type_id, "params": params}))
            }
            "set_instrument_param" => {
                let id = self.resolve_channel(a.get("channel"))?;
                let key = s(a, "param").ok_or("missing `param`")?;
                let value = f(a, "value").ok_or("missing `value`")? as f32;
                let ty = self.project.channel(id).unwrap().instrument.type_id.clone();
                let info = soundesti_session::instrument_params(&ty).iter().find(|p| p.key == key).ok_or_else(|| format!("no parameter `{key}`"))?;
                let v = value.clamp(info.min, info.max);
                self.checkpoint("Instrument parameter (AI)");
                self.project.channel_mut(id).unwrap().instrument.params.insert(key.to_string(), v);
                text(json!({"ok": true, "value": v}))
            }
            "set_mixer_track" => {
                let t = i(a, "track").ok_or("missing `track`")?;
                if t < 0 || t as usize >= self.project.mixer.len() {
                    return Err(format!("track must be 0-{}", self.project.mixer.len() - 1));
                }
                let t = t as usize;
                self.checkpoint("Mixer (AI)");
                let tr = &mut self.project.mixer[t];
                if let Some(v) = f(a, "volume") {
                    tr.volume = (v as f32).clamp(0.0, 1.25);
                }
                if let Some(p) = f(a, "pan") {
                    tr.pan = (p as f32).clamp(-1.0, 1.0);
                }
                if let Some(m) = b(a, "muted") {
                    tr.muted = m;
                }
                if let Some(n) = s(a, "name") {
                    tr.name = n.to_string();
                }
                if let Some(solo) = b(a, "solo") {
                    self.mixer_solo = if solo && t > 0 { Some(t) } else { None };
                }
                self.selected_track = t;
                text(json!({"ok": true}))
            }
            "pattern" => match s(a, "action").ok_or("missing `action`")? {
                "select" => {
                    let pid = i(a, "pattern").ok_or("select needs `pattern`")?;
                    let p = self.project.patterns.iter().find(|p| p.id.0 as i64 == pid).ok_or("no such pattern")?;
                    self.project.current_pattern = p.id;
                    self.needs_sync = true;
                    text(json!({"ok": true}))
                }
                "new" => {
                    self.checkpoint("New pattern (AI)");
                    let id = self.project.add_pattern();
                    if let Some(n) = s(a, "name") {
                        self.project.pattern_mut(id).unwrap().name = n.to_string();
                    }
                    self.project.current_pattern = id;
                    text(json!({"ok": true, "pattern": id.0}))
                }
                "rename" => {
                    let pid = i(a, "pattern").map(|x| soundesti_project::PatternId(x as u32)).unwrap_or(self.project.current_pattern);
                    let name = s(a, "name").ok_or("rename needs `name`")?;
                    self.checkpoint("Rename pattern (AI)");
                    self.project.pattern_mut(pid).ok_or("no such pattern")?.name = name.to_string();
                    text(json!({"ok": true}))
                }
                other => Err(format!("unknown action `{other}`")),
            },
            "set_window" => {
                let open = b(a, "open").ok_or("missing `open`")?;
                let w = &mut self.win;
                let slot = match s(a, "window").ok_or("missing `window`")? {
                    "channel_rack" => &mut w.channel_rack,
                    "piano_roll" => &mut w.piano_roll,
                    "mixer" => &mut w.mixer,
                    "playlist" => &mut w.playlist,
                    "browser" => &mut w.browser,
                    "plugin_picker" => &mut w.plugin_picker,
                    "project_info" => &mut w.song_info,
                    "midi_settings" => &mut w.midi_settings,
                    "audio_settings" => &mut w.audio_settings,
                    "shortcuts" => &mut w.help,
                    "quantizer" => &mut w.quantize,
                    "export" => &mut w.export,
                    "ai_setup" => &mut w.ai_setup,
                    other => return Err(format!("unknown window `{other}`")),
                };
                *slot = open;
                text(json!({"ok": true}))
            }
            "save_project" => {
                let path = match s(a, "path") {
                    Some(p) => PathBuf::from(p),
                    None => self.path.clone().ok_or("project was never saved; pass `path`")?,
                };
                if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case(soundesti_project::FILE_EXTENSION)) {
                    return Err("path must end in .soundesti".into());
                }
                if self.save_to(path.clone()) { text(json!({"ok": true, "path": path})) } else { Err("save failed (see the dialog in Soundesti)".into()) }
            }
            "open_project" | "new_project" => {
                if self.dirty() && !b(a, "discard_unsaved").unwrap_or(false) {
                    return Err("there are unsaved changes; save first or pass discard_unsaved=true".into());
                }
                let after = if name == "new_project" {
                    AfterConfirm::New
                } else {
                    let p = PathBuf::from(s(a, "path").ok_or("missing `path`")?);
                    if !p.exists() {
                        return Err(format!("{} does not exist", p.display()));
                    }
                    AfterConfirm::Open(Some(p))
                };
                self.perform(ctx, after);
                text(json!({"ok": true, "status": self.status}))
            }
            other => Err(format!("tool `{other}` is not handled")),
        }
        .inspect(|_| self.needs_sync = true)
    }

    fn state_json(&self) -> Value {
        let ts = self.project.time_signature;
        let pos = self.shared.position_tick();
        let cur = self.project.current();
        json!({
            "file": self.path,
            "unsaved_changes": self.dirty(),
            "transport": {
                "playing": self.shared.playing.load(std::sync::atomic::Ordering::Relaxed),
                "position_ticks": pos,
                "position_bst": BarStepTick::from_tick(pos.max(0.0) as Tick, ts).to_string(),
                "mode": "pattern",
            },
            "tempo_bpm": self.project.bpm,
            "time_signature": format!("{}/{}", ts.numerator, ts.denominator),
            "metronome": self.project.metronome,
            "snap_ticks": self.snap,
            "typing_keyboard": self.typing_piano,
            "keyboard_focus": self.focus.id(),
            "current_pattern": {"id": cur.id.0, "name": cur.name, "length_ticks": cur.length_ticks(ts)},
            "patterns": self.project.patterns.iter().map(|p| json!({"id": p.id.0, "name": p.name})).collect::<Vec<_>>(),
            "selected_channel": self.selected_channel.map(|c| c.0),
            "channels": self.project.channels.iter().map(|c| json!({
                "id": c.id.0, "name": c.name, "instrument": c.instrument.type_id,
                "volume": c.volume, "pan": c.pan, "muted": c.muted, "solo": self.session.solo.contains(&c.id),
                "mixer_track": c.mixer_track, "notes_in_pattern": cur.notes(c.id).len(),
            })).collect::<Vec<_>>(),
            "mixer": self.project.mixer.iter().enumerate()
                .filter(|(i, t)| *i == 0 || self.project.channels.iter().any(|c| c.mixer_track as usize == *i) || t.muted || t.volume != 0.8 || t.pan != 0.0)
                .map(|(i, t)| json!({"track": i, "name": t.name, "volume": t.volume, "pan": t.pan, "muted": t.muted}))
                .collect::<Vec<_>>(),
            "mixer_solo": self.mixer_solo,
            "windows": {
                "channel_rack": self.win.channel_rack, "piano_roll": self.win.piano_roll, "mixer": self.win.mixer,
                "playlist": self.win.playlist, "browser": self.win.browser,
            },
            "audio": {"device": self.output.info.device, "sample_rate": self.output.info.sample_rate, "cpu_load": self.shared.cpu_load()},
            "status": self.status,
            "time_units": {"ppq": PPQ, "ticks_per_step": TICKS_PER_STEP},
        })
    }

    fn mcp_screenshot(&mut self, ctx: &egui::Context, reply: Sender<ToolResult>) {
        let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
            let [w, h] = img.size;
            let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
            let res = (|| -> Result<ToolOutput, String> {
                let mut rgba = image::RgbaImage::from_raw(w as u32, h as u32, bytes).ok_or("bad screenshot buffer")?;
                if w > 1600 {
                    let nh = (h as f64 * 1600.0 / w as f64) as u32;
                    rgba = image::imageops::resize(&rgba, 1600, nh, image::imageops::FilterType::Triangle);
                }
                let mut png = Vec::new();
                image::DynamicImage::ImageRgba8(rgba)
                    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                    .map_err(|e| e.to_string())?;
                Ok(ToolOutput::Image { png, note: format!("Soundesti window, original size {w}x{h}") })
            })();
            let _ = reply.send(res);
        });
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
        ctx.request_repaint();
    }

    fn mcp_export(&mut self, a: &Value, reply: Sender<ToolResult>) {
        let Some(path) = s(a, "path").map(PathBuf::from) else {
            let _ = reply.send(Err("missing `path`".into()));
            return;
        };
        if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("wav")) {
            let _ = reply.send(Err("path must end in .wav".into()));
            return;
        }
        let mut opts = soundesti_export::ExportOptions::default();
        if let Some(l) = i(a, "loops") {
            opts.loops = l.clamp(1, 64) as u32;
        }
        if let Some(t) = f(a, "tail_seconds") {
            opts.tail_seconds = t.clamp(0.0, 30.0) as f32;
        }
        if let Some(sr) = i(a, "sample_rate") {
            opts.sample_rate = sr as u32;
        }
        opts.bit_depth = match s(a, "bit_depth") {
            Some("16") => soundesti_export::BitDepth::Int16,
            Some("32f") => soundesti_export::BitDepth::Float32,
            _ => soundesti_export::BitDepth::Int24,
        };
        let project = self.project.clone();
        std::thread::spawn(move || {
            let res = soundesti_export::export_wav(&project, &path, &opts, |_| true)
                .map(|r| {
                    ToolOutput::Text(
                        json!({"ok": true, "path": path, "seconds": r.seconds, "peak": r.peak}).to_string(),
                    )
                })
                .map_err(|e| e.to_string());
            let _ = reply.send(res);
        });
    }
}
