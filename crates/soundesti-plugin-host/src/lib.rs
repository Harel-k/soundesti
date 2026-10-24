//! Third-party plugin hosting (VST2, VST3, CLAP).
//!
//! A loaded plugin is a [`PluginInstance`]: one object shared (through `Arc`) between the
//! audio thread, which only calls [`PluginInstance::process`], and the UI thread, which
//! opens the editor, reads parameters and saves state. That split mirrors the threading
//! rules of the plugin formats themselves. [`PluginInstrument`] and [`PluginEffect`]
//! adapt an instance to Soundesti's engine traits.
//!
//! Plugins run inside the Soundesti process: a plugin that crashes takes Soundesti with
//! it. Scanning, the riskiest step, runs in a child process (see [`scan`]).

pub mod editor;
pub mod scan;
mod vst2;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use soundesti_plugin_api::{Effect, Instrument, NoteEvent, ParamInfo, TimedEvent};

/// Largest block a plugin is asked to render at once; longer blocks are split.
pub const MAX_BLOCK: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PluginFormat {
    Vst2,
    Vst3,
    Clap,
}

impl PluginFormat {
    pub fn label(self) -> &'static str {
        match self {
            PluginFormat::Vst2 => "VST",
            PluginFormat::Vst3 => "VST3",
            PluginFormat::Clap => "CLAP",
        }
    }
}

/// What the scanner learned about one plugin (saved in the plugin cache and in projects).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PluginDesc {
    pub format: PluginFormat,
    pub path: PathBuf,
    /// Format-specific identity: VST2 unique ID, VST3 class ID or CLAP plugin ID.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vendor: String,
    pub is_instrument: bool,
    #[serde(default)]
    pub inputs: u32,
    #[serde(default)]
    pub outputs: u32,
}

#[derive(Debug)]
pub enum HostError {
    Load(String),
    NotAPlugin(String),
    Failed(String),
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostError::Load(m) => write!(f, "could not load the plugin file: {m}"),
            HostError::NotAPlugin(m) => write!(f, "not a supported plugin: {m}"),
            HostError::Failed(m) => write!(f, "the plugin failed to start: {m}"),
        }
    }
}

impl std::error::Error for HostError {}

/// A running plugin. UI-thread methods and `process` (audio thread) may run concurrently,
/// exactly as the plugin formats allow.
pub trait PluginInstance: Send + Sync {
    fn desc(&self) -> &PluginDesc;
    fn param_count(&self) -> usize;
    fn param_name(&self, index: usize) -> String;
    /// Normalized 0..1.
    fn param_value(&self, index: usize) -> f32;
    fn param_display(&self, index: usize) -> String;
    fn set_param_value(&self, index: usize, value: f32);
    /// The plugin's complete settings, to save in the project.
    fn save_state(&self) -> Option<Vec<u8>>;
    fn load_state(&self, data: &[u8]) -> bool;
    fn has_editor(&self) -> bool;
    /// Opens the editor inside `parent` (a native window handle); returns its size.
    fn open_editor(&self, parent: isize) -> Option<(i32, i32)>;
    fn close_editor(&self);
    /// Called regularly on the UI thread while the editor is open.
    fn editor_idle(&self) {}
    /// Audio thread only. Replaces `left`/`right` with the plugin's output; for effects they
    /// hold the input on entry. `events` are sorted and inside the block.
    fn process(&self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]);
}

static ANY_LOADED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// True once any plugin code has been loaded into this process.
pub fn any_loaded() -> bool {
    ANY_LOADED.load(std::sync::atomic::Ordering::Relaxed)
}

/// Ends the process without running DLL shutdown code. Plugins such as FL Studio VSTi crash
/// in their own shutdown when unloaded at exit; everything of ours is saved by then.
pub fn exit_skipping_plugin_shutdown(code: u32) -> ! {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    unsafe { TerminateProcess(GetCurrentProcess(), code) };
    std::process::exit(code as i32)
}

/// Loads and starts a plugin described by a scan result.
pub fn load(desc: &PluginDesc, sample_rate: f32) -> Result<Arc<dyn PluginInstance>, HostError> {
    match desc.format {
        PluginFormat::Vst2 => {
            ANY_LOADED.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(Arc::new(vst2::Vst2Plugin::load(desc, sample_rate)?))
        }
        PluginFormat::Vst3 | PluginFormat::Clap => {
            Err(HostError::NotAPlugin(format!("{} hosting is not available yet", desc.format.label())))
        }
    }
}

/// Inspects one plugin file (used by the scanner child process).
pub fn describe(format: PluginFormat, path: &Path) -> Result<Vec<PluginDesc>, HostError> {
    match format {
        PluginFormat::Vst2 => vst2::describe(path).map(|d| vec![d]),
        PluginFormat::Vst3 | PluginFormat::Clap => {
            Err(HostError::NotAPlugin(format!("{} hosting is not available yet", format.label())))
        }
    }
}

/// Converts note events to MIDI bytes, tracking which key each note ID played.
pub(crate) struct NoteMap {
    keys: [(u32, u8); 256],
    len: usize,
}

impl NoteMap {
    pub fn new() -> Self {
        Self { keys: [(0, 0); 256], len: 0 }
    }

    /// Returns (status, data1, data2) MIDI bytes for an event on channel 1.
    pub fn midi(&mut self, e: &NoteEvent) -> Option<[u8; 3]> {
        match *e {
            NoteEvent::On { id, key, velocity } => {
                if self.len < self.keys.len() {
                    self.keys[self.len] = (id, key);
                    self.len += 1;
                }
                Some([0x90, key.min(127), ((velocity * 127.0).round() as u8).clamp(1, 127)])
            }
            NoteEvent::Off { id } => {
                let i = self.keys[..self.len].iter().position(|(k, _)| *k == id)?;
                let key = self.keys[i].1;
                self.keys[i] = self.keys[self.len - 1];
                self.len -= 1;
                Some([0x80, key, 64])
            }
            NoteEvent::AllNotesOff => {
                self.len = 0;
                Some([0xB0, 123, 0])
            }
            NoteEvent::AllSoundOff => {
                self.len = 0;
                Some([0xB0, 120, 0])
            }
        }
    }
}

/// A hosted instrument plugin on a Channel Rack channel.
pub struct PluginInstrument {
    pub plugin: Arc<dyn PluginInstance>,
    voices: usize,
    left: Vec<f32>,
    right: Vec<f32>,
    events: Vec<TimedEvent>,
}

impl PluginInstrument {
    pub fn new(plugin: Arc<dyn PluginInstance>) -> Self {
        Self {
            plugin,
            voices: 0,
            left: vec![0.0; MAX_BLOCK],
            right: vec![0.0; MAX_BLOCK],
            events: Vec::with_capacity(1024),
        }
    }
}

impl Instrument for PluginInstrument {
    fn type_id(&self) -> &'static str {
        PLUGIN_TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Plugin"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &[]
    }
    fn get_param(&self, id: u32) -> f32 {
        self.plugin.param_value(id as usize)
    }
    fn set_param(&mut self, id: u32, value: f32) {
        self.plugin.set_param_value(id as usize, value);
    }
    fn process(&mut self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]) {
        for e in events {
            match e.event {
                NoteEvent::On { .. } => self.voices += 1,
                NoteEvent::Off { .. } => self.voices = self.voices.saturating_sub(1),
                _ => self.voices = 0,
            }
        }
        // Plugins replace their output; the engine expects instruments to add into it.
        let mut start = 0;
        while start < left.len() {
            let n = (left.len() - start).min(MAX_BLOCK);
            self.events.clear();
            for e in events.iter().filter(|e| (start..start + n).contains(&(e.offset as usize))) {
                if self.events.len() < self.events.capacity() {
                    self.events.push(TimedEvent { offset: e.offset - start as u32, event: e.event });
                }
            }
            let (l, r) = (&mut self.left[..n], &mut self.right[..n]);
            l.fill(0.0);
            r.fill(0.0);
            self.plugin.process(&self.events, l, r);
            for i in 0..n {
                left[start + i] += l[i];
                right[start + i] += r[i];
            }
            start += n;
        }
    }
    fn active_voices(&self) -> usize {
        self.voices
    }
}
/// A hosted effect plugin in a mixer insert slot.
pub struct PluginEffect {
    pub plugin: Arc<dyn PluginInstance>,
}

impl Effect for PluginEffect {
    fn type_id(&self) -> &'static str {
        PLUGIN_TYPE_ID
    }
    fn display_name(&self) -> &'static str {
        "Plugin"
    }
    fn params(&self) -> &'static [ParamInfo] {
        &[]
    }
    fn get_param(&self, id: u32) -> f32 {
        self.plugin.param_value(id as usize)
    }
    fn set_param(&mut self, id: u32, value: f32) {
        self.plugin.set_param_value(id as usize, value);
    }
    fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        for (l, r) in left.chunks_mut(MAX_BLOCK).zip(right.chunks_mut(MAX_BLOCK)) {
            self.plugin.process(&[], l, r);
        }
    }
}

/// `type_id` that marks a channel or effect slot as a hosted plugin.
pub const PLUGIN_TYPE_ID: &str = "plugin";
