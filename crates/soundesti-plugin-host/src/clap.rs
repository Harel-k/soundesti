//! CLAP hosting through the `clap-sys` bindings (MIT/Apache).

use std::cell::UnsafeCell;
use std::collections::HashSet;
use std::ffi::{CStr, CString, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::{CLAP_EXT_AUDIO_PORTS, clap_audio_port_info, clap_plugin_audio_ports};
use clap_sys::ext::gui::{
    CLAP_EXT_GUI, CLAP_WINDOW_API_WIN32, clap_host_gui, clap_plugin_gui, clap_window, clap_window_handle,
};
use clap_sys::ext::params::{CLAP_EXT_PARAMS, clap_param_info, clap_plugin_params};
use clap_sys::ext::state::{CLAP_EXT_STATE, clap_plugin_state};
use clap_sys::factory::plugin_factory::{CLAP_PLUGIN_FACTORY_ID, clap_plugin_factory};
use clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR;
use clap_sys::host::clap_host;
use clap_sys::plugin::clap_plugin;
use clap_sys::process::clap_process;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;
use soundesti_plugin_api::{TimedEvent, transport};

use crate::{HostError, MAX_BLOCK, NoteMap, PluginDesc, PluginFormat, PluginInstance, editor};

const MAX_EVENTS: usize = 512;
const MAX_PARAM_EVENTS: usize = 64;

/// Libraries whose `clap_entry.init` already ran (it must run once per process).
static INITIALIZED: Mutex<Option<HashSet<PathBuf>>> = Mutex::new(None);

fn cstr_opt(p: *const c_char) -> String {
    if p.is_null() { String::new() } else { unsafe { CStr::from_ptr(p) }.to_string_lossy().trim().to_string() }
}

struct Library {
    _lib: std::mem::ManuallyDrop<libloading::Library>,
    factory: *const clap_plugin_factory,
}

fn open(path: &Path) -> Result<Library, HostError> {
    let lib = unsafe { libloading::Library::new(path) }.map_err(|e| HostError::Load(e.to_string()))?;
    let entry: *const clap_plugin_entry = unsafe {
        *lib.get::<*const clap_plugin_entry>(b"clap_entry\0")
            .map_err(|_| HostError::NotAPlugin("no CLAP entry point".into()))?
    };
    if entry.is_null() {
        return Err(HostError::NotAPlugin("empty CLAP entry".into()));
    }
    let e = unsafe { &*entry };
    let mut done = INITIALIZED.lock().map_err(|_| HostError::Failed("lock".into()))?;
    let set = done.get_or_insert_with(HashSet::new);
    if !set.contains(path) {
        let cpath = CString::new(path.display().to_string()).unwrap_or_default();
        if let Some(init) = e.init
            && !unsafe { init(cpath.as_ptr()) }
        {
            return Err(HostError::Failed("the plugin library refused to start".into()));
        }
        set.insert(path.to_path_buf());
    }
    let factory = match e.get_factory {
        Some(f) => (unsafe { f(CLAP_PLUGIN_FACTORY_ID.as_ptr()) }) as *const clap_plugin_factory,
        None => std::ptr::null(),
    };
    if factory.is_null() {
        return Err(HostError::NotAPlugin("no plugin factory".into()));
    }
    // Never unloaded (see vst2.rs).
    Ok(Library { _lib: std::mem::ManuallyDrop::new(lib), factory })
}

pub fn describe(path: &Path) -> Result<Vec<PluginDesc>, HostError> {
    let lib = open(path)?;
    let f = unsafe { &*lib.factory };
    let count = f.get_plugin_count.map(|c| unsafe { c(lib.factory) }).unwrap_or(0);
    let mut out = Vec::new();
    for i in 0..count {
        let Some(get) = f.get_plugin_descriptor else { break };
        let d = unsafe { get(lib.factory, i) };
        if d.is_null() {
            continue;
        }
        let d = unsafe { &*d };
        let mut features = Vec::new();
        let mut p = d.features;
        while !p.is_null() && unsafe { !(*p).is_null() } && features.len() < 64 {
            features.push(cstr_opt(unsafe { *p }));
            p = unsafe { p.add(1) };
        }
        out.push(PluginDesc {
            format: PluginFormat::Clap,
            path: path.to_path_buf(),
            id: cstr_opt(d.id),
            name: cstr_opt(d.name),
            vendor: cstr_opt(d.vendor),
            is_instrument: features.iter().any(|f| f == "instrument"),
            inputs: 0,
            outputs: 0,
        });
    }
    if out.is_empty() {
        return Err(HostError::NotAPlugin("no plugins in this file".into()));
    }
    Ok(out)
}

/// Per-instance host data, reachable from the `clap_host` the plugin was created with.
struct HostData {
    editor_hwnd: AtomicIsize,
    callback_requested: AtomicBool,
}

unsafe extern "C" fn host_get_extension(_host: *const clap_host, id: *const c_char) -> *const c_void {
    if !id.is_null() && unsafe { CStr::from_ptr(id) } == CLAP_EXT_GUI {
        return &HOST_GUI as *const clap_host_gui as *const c_void;
    }
    std::ptr::null()
}
unsafe extern "C" fn host_request_restart(_host: *const clap_host) {}
unsafe extern "C" fn host_request_process(_host: *const clap_host) {}
unsafe extern "C" fn host_request_callback(host: *const clap_host) {
    let data = unsafe { &*((*host).host_data as *const HostData) };
    data.callback_requested.store(true, Ordering::Release);
}

unsafe extern "C" fn gui_resize_hints_changed(_host: *const clap_host) {}
unsafe extern "C" fn gui_request_resize(host: *const clap_host, width: u32, height: u32) -> bool {
    let data = unsafe { &*((*host).host_data as *const HostData) };
    let hwnd = data.editor_hwnd.load(Ordering::Acquire);
    if hwnd != 0 {
        editor::set_client_size(hwnd, width as i32, height as i32);
    }
    true
}
unsafe extern "C" fn gui_request_show(_host: *const clap_host) -> bool {
    true
}
unsafe extern "C" fn gui_request_hide(_host: *const clap_host) -> bool {
    true
}
unsafe extern "C" fn gui_closed(_host: *const clap_host, _was_destroyed: bool) {}

static HOST_GUI: clap_host_gui = clap_host_gui {
    resize_hints_changed: Some(gui_resize_hints_changed),
    request_resize: Some(gui_request_resize),
    request_show: Some(gui_request_show),
    request_hide: Some(gui_request_hide),
    closed: Some(gui_closed),
};

/// Input event list for one process call (audio thread only).
struct InEvents {
    notes: Vec<clap_event_note>,
    params: Vec<clap_event_param_value>,
    order: Vec<*const clap_event_header>,
}

unsafe extern "C" fn in_size(list: *const clap_input_events) -> u32 {
    let ev = unsafe { &*((*list).ctx as *const InEvents) };
    ev.order.len() as u32
}
unsafe extern "C" fn in_get(list: *const clap_input_events, index: u32) -> *const clap_event_header {
    let ev = unsafe { &*((*list).ctx as *const InEvents) };
    ev.order.get(index as usize).copied().unwrap_or(std::ptr::null())
}
unsafe extern "C" fn out_try_push(_list: *const clap_output_events, _event: *const clap_event_header) -> bool {
    true // output events (e.g. parameter gestures) are ignored
}

struct ByteStream {
    data: Vec<u8>,
    pos: usize,
}

unsafe extern "C" fn ostream_write(stream: *const clap_ostream, buffer: *const c_void, size: u64) -> i64 {
    let s = unsafe { &mut *((*stream).ctx as *mut ByteStream) };
    s.data.extend_from_slice(unsafe { std::slice::from_raw_parts(buffer as *const u8, size as usize) });
    size as i64
}
unsafe extern "C" fn istream_read(stream: *const clap_istream, buffer: *mut c_void, size: u64) -> i64 {
    let s = unsafe { &mut *((*stream).ctx as *mut ByteStream) };
    let n = (size as usize).min(s.data.len() - s.pos);
    unsafe { std::ptr::copy_nonoverlapping(s.data.as_ptr().add(s.pos), buffer as *mut u8, n) };
    s.pos += n;
    n as i64
}

struct AudioState {
    notes: NoteMap,
    events: InEvents,
    in_bufs: Vec<Vec<Vec<f32>>>,
    out_bufs: Vec<Vec<Vec<f32>>>,
    in_ptrs: Vec<Vec<*mut f32>>,
    out_ptrs: Vec<Vec<*mut f32>>,
    in_ports: Vec<clap_audio_buffer>,
    out_ports: Vec<clap_audio_buffer>,
    transport: clap_event_transport,
    steady: i64,
    started: bool,
}

pub struct ClapPlugin {
    desc: PluginDesc,
    plugin: *const clap_plugin,
    // Boxed so their addresses stay fixed for the plugin's lifetime.
    _host: Box<clap_host>,
    host_data: Box<HostData>,
    _strings: [CString; 4],
    params_ext: *const clap_plugin_params,
    state_ext: *const clap_plugin_state,
    gui_ext: *const clap_plugin_gui,
    params: Vec<(u32, String, f64, f64)>,
    pending: Mutex<Vec<(u32, f64)>>,
    gui_open: AtomicBool,
    audio: UnsafeCell<AudioState>,
    _lib: Library,
}

// CLAP's threading rules are followed: process on the audio thread, everything else on
// the UI thread.
unsafe impl Send for ClapPlugin {}
unsafe impl Sync for ClapPlugin {}

unsafe fn ext<T>(plugin: *const clap_plugin, id: &CStr) -> *const T {
    match unsafe { (*plugin).get_extension } {
        Some(f) => (unsafe { f(plugin, id.as_ptr()) }) as *const T,
        None => std::ptr::null(),
    }
}

fn port_channels(plugin: *const clap_plugin, ports: *const clap_plugin_audio_ports, input: bool) -> Vec<usize> {
    if ports.is_null() {
        // No audio-ports extension: assume stereo out (and stereo in for effects).
        return vec![2];
    }
    let p = unsafe { &*ports };
    let n = p.count.map(|c| unsafe { c(plugin, input) }).unwrap_or(0).min(16);
    (0..n)
        .map(|i| {
            let mut info: clap_audio_port_info = unsafe { std::mem::zeroed() };
            let ok = p.get.map(|g| unsafe { g(plugin, i, input, &mut info) }).unwrap_or(false);
            if ok { info.channel_count.clamp(1, 32) as usize } else { 2 }
        })
        .collect()
}

impl ClapPlugin {
    pub fn load(desc: &PluginDesc, sample_rate: f32) -> Result<Self, HostError> {
        let lib = open(&desc.path)?;
        let strings = [
            CString::new("Soundesti").unwrap(),
            CString::new("Soundesti").unwrap(),
            CString::new("https://github.com/Harel-k/soundesti").unwrap(),
            CString::new(env!("CARGO_PKG_VERSION")).unwrap(),
        ];
        let host_data =
            Box::new(HostData { editor_hwnd: AtomicIsize::new(0), callback_requested: AtomicBool::new(false) });
        let host = Box::new(clap_host {
            clap_version: CLAP_VERSION,
            host_data: &*host_data as *const HostData as *mut c_void,
            name: strings[0].as_ptr(),
            vendor: strings[1].as_ptr(),
            url: strings[2].as_ptr(),
            version: strings[3].as_ptr(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request_restart),
            request_process: Some(host_request_process),
            request_callback: Some(host_request_callback),
        });
        let id = CString::new(desc.id.clone()).map_err(|_| HostError::NotAPlugin("bad plugin ID".into()))?;
        let f = unsafe { &*lib.factory };
        let plugin = match f.create_plugin {
            Some(c) => unsafe { c(lib.factory, &*host, id.as_ptr()) },
            None => std::ptr::null(),
        };
        if plugin.is_null() {
            return Err(HostError::Failed("could not create the plugin".into()));
        }
        unsafe {
            let p = &*plugin;
            if !p.init.map(|i| i(plugin)).unwrap_or(false) {
                if let Some(d) = p.destroy {
                    d(plugin);
                }
                return Err(HostError::Failed("the plugin refused to initialize".into()));
            }
            let ports: *const clap_plugin_audio_ports = ext(plugin, CLAP_EXT_AUDIO_PORTS);
            let ins =
                if desc.is_instrument && ports.is_null() { Vec::new() } else { port_channels(plugin, ports, true) };
            let outs = port_channels(plugin, ports, false);
            if !p.activate.map(|a| a(plugin, sample_rate as f64, 1, MAX_BLOCK as u32)).unwrap_or(false) {
                if let Some(d) = p.destroy {
                    d(plugin);
                }
                return Err(HostError::Failed("the plugin could not start audio".into()));
            }
            let params_ext: *const clap_plugin_params = ext(plugin, CLAP_EXT_PARAMS);
            let mut params = Vec::new();
            if !params_ext.is_null() {
                let pe = &*params_ext;
                for i in 0..pe.count.map(|c| c(plugin)).unwrap_or(0).min(100_000) {
                    let mut info: clap_param_info = std::mem::zeroed();
                    if pe.get_info.map(|g| g(plugin, i, &mut info)).unwrap_or(false)
                        && info.flags & clap_sys::ext::params::CLAP_PARAM_IS_HIDDEN == 0
                    {
                        params.push((info.id, cstr_opt(info.name.as_ptr()), info.min_value, info.max_value));
                    }
                }
            }
            let mk = |chs: &[usize]| -> Vec<Vec<Vec<f32>>> {
                chs.iter().map(|&c| (0..c).map(|_| vec![0.0; MAX_BLOCK]).collect()).collect()
            };
            let (in_bufs, out_bufs) = (mk(&ins), mk(&outs));
            let port = |c: usize| clap_audio_buffer {
                data32: std::ptr::null_mut(),
                data64: std::ptr::null_mut(),
                channel_count: c as u32,
                latency: 0,
                constant_mask: 0,
            };
            let audio = AudioState {
                notes: NoteMap::new(),
                events: InEvents {
                    notes: Vec::with_capacity(MAX_EVENTS),
                    params: Vec::with_capacity(MAX_PARAM_EVENTS),
                    order: Vec::with_capacity(MAX_EVENTS + MAX_PARAM_EVENTS),
                },
                in_ptrs: in_bufs.iter().map(|b| vec![std::ptr::null_mut(); b.len()]).collect(),
                out_ptrs: out_bufs.iter().map(|b| vec![std::ptr::null_mut(); b.len()]).collect(),
                in_ports: ins.iter().map(|&c| port(c)).collect(),
                out_ports: outs.iter().map(|&c| port(c)).collect(),
                in_bufs,
                out_bufs,
                transport: std::mem::zeroed(),
                steady: 0,
                started: false,
            };
            let mut desc = desc.clone();
            desc.inputs = ins.iter().sum::<usize>() as u32;
            desc.outputs = outs.iter().sum::<usize>() as u32;
            Ok(Self {
                desc,
                plugin,
                _host: host,
                host_data,
                _strings: strings,
                params_ext,
                state_ext: ext(plugin, CLAP_EXT_STATE),
                gui_ext: ext(plugin, CLAP_EXT_GUI),
                params,
                pending: Mutex::new(Vec::new()),
                gui_open: AtomicBool::new(false),
                audio: UnsafeCell::new(audio),
                _lib: lib,
            })
        }
    }

    fn plain_value(&self, id: u32) -> Option<f64> {
        if self.params_ext.is_null() {
            return None;
        }
        let mut v = 0.0;
        unsafe { (*self.params_ext).get_value.map(|g| g(self.plugin, id, &mut v)).unwrap_or(false) }.then_some(v)
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        self.close_editor();
        unsafe {
            let p = &*self.plugin;
            if (*self.audio.get()).started
                && let Some(s) = p.stop_processing
            {
                s(self.plugin);
            }
            if let Some(d) = p.deactivate {
                d(self.plugin);
            }
            if let Some(d) = p.destroy {
                d(self.plugin);
            }
        }
    }
}

const STATE_MAGIC: &[u8; 4] = b"SDCL";

impl PluginInstance for ClapPlugin {
    fn desc(&self) -> &PluginDesc {
        &self.desc
    }

    fn param_count(&self) -> usize {
        self.params.len()
    }

    fn param_name(&self, index: usize) -> String {
        self.params.get(index).map(|p| p.1.clone()).unwrap_or_default()
    }

    fn param_value(&self, index: usize) -> f32 {
        let Some(&(id, _, min, max)) = self.params.get(index) else { return 0.0 };
        let v = self.plain_value(id).unwrap_or(min);
        if max > min { ((v - min) / (max - min)) as f32 } else { 0.0 }
    }

    fn param_display(&self, index: usize) -> String {
        let Some(&(id, ..)) = self.params.get(index) else { return String::new() };
        let Some(v) = self.plain_value(id) else { return String::new() };
        let mut buf = [0 as c_char; 128];
        let ok = unsafe {
            (*self.params_ext)
                .value_to_text
                .map(|t| t(self.plugin, id, v, buf.as_mut_ptr(), buf.len() as u32))
                .unwrap_or(false)
        };
        if ok { cstr_opt(buf.as_ptr()) } else { format!("{v:.3}") }
    }

    fn set_param_value(&self, index: usize, value: f32) {
        let Some(&(id, _, min, max)) = self.params.get(index) else { return };
        let plain = min + (max - min) * value.clamp(0.0, 1.0) as f64;
        if let Ok(mut p) = self.pending.lock() {
            p.retain(|(i, _)| *i != id);
            p.push((id, plain));
        }
    }

    fn save_state(&self) -> Option<Vec<u8>> {
        if self.state_ext.is_null() {
            return None;
        }
        let mut s = ByteStream { data: STATE_MAGIC.to_vec(), pos: 0 };
        let os = clap_ostream { ctx: &mut s as *mut ByteStream as *mut c_void, write: Some(ostream_write) };
        unsafe { (*self.state_ext).save.map(|f| f(self.plugin, &os)).unwrap_or(false) }.then_some(s.data)
    }

    fn load_state(&self, data: &[u8]) -> bool {
        let Some(rest) = data.strip_prefix(STATE_MAGIC) else { return false };
        if self.state_ext.is_null() {
            return false;
        }
        let mut s = ByteStream { data: rest.to_vec(), pos: 0 };
        let is = clap_istream { ctx: &mut s as *mut ByteStream as *mut c_void, read: Some(istream_read) };
        unsafe { (*self.state_ext).load.map(|f| f(self.plugin, &is)).unwrap_or(false) }
    }

    fn has_editor(&self) -> bool {
        !self.gui_ext.is_null()
            && unsafe {
                (*self.gui_ext)
                    .is_api_supported
                    .map(|f| f(self.plugin, CLAP_WINDOW_API_WIN32.as_ptr(), false))
                    .unwrap_or(false)
            }
    }

    fn open_editor(&self, parent: isize) -> Option<(i32, i32)> {
        if !self.has_editor() {
            return None;
        }
        let g = unsafe { &*self.gui_ext };
        unsafe {
            if !g.create.map(|c| c(self.plugin, CLAP_WINDOW_API_WIN32.as_ptr(), false)).unwrap_or(false) {
                return None;
            }
            self.host_data.editor_hwnd.store(parent, Ordering::Release);
            let window = clap_window {
                api: CLAP_WINDOW_API_WIN32.as_ptr(),
                specific: clap_window_handle { win32: parent as *mut c_void },
            };
            if !g.set_parent.map(|s| s(self.plugin, &window)).unwrap_or(false) {
                if let Some(d) = g.destroy {
                    d(self.plugin);
                }
                return None;
            }
            let (mut w, mut h) = (640u32, 480u32);
            g.get_size.map(|s| s(self.plugin, &mut w, &mut h));
            if let Some(s) = g.show {
                s(self.plugin);
            }
            self.gui_open.store(true, Ordering::Release);
            Some((w.max(100) as i32, h.max(60) as i32))
        }
    }

    fn close_editor(&self) {
        if self.gui_open.swap(false, Ordering::AcqRel) {
            let g = unsafe { &*self.gui_ext };
            unsafe {
                if let Some(h) = g.hide {
                    h(self.plugin);
                }
                if let Some(d) = g.destroy {
                    d(self.plugin);
                }
            }
            self.host_data.editor_hwnd.store(0, Ordering::Release);
        }
    }

    fn editor_idle(&self) {
        self.main_thread_idle();
    }

    fn main_thread_idle(&self) {
        if self.host_data.callback_requested.swap(false, Ordering::AcqRel)
            && let Some(f) = unsafe { (*self.plugin).on_main_thread }
        {
            unsafe { f(self.plugin) };
        }
    }

    fn process(&self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]) {
        let st = unsafe { &mut *self.audio.get() };
        let n = left.len().min(right.len()).min(MAX_BLOCK);
        let p = unsafe { &*self.plugin };
        if !st.started {
            st.started = p.start_processing.map(|s| unsafe { s(self.plugin) }).unwrap_or(true);
        }

        // Events: parameter changes first (at time 0), then notes in order.
        let ev = &mut st.events;
        ev.notes.clear();
        ev.params.clear();
        ev.order.clear();
        if let Ok(mut pending) = self.pending.try_lock() {
            for (id, value) in pending.drain(..).take(MAX_PARAM_EVENTS) {
                ev.params.push(clap_event_param_value {
                    header: clap_event_header {
                        size: std::mem::size_of::<clap_event_param_value>() as u32,
                        time: 0,
                        space_id: CLAP_CORE_EVENT_SPACE_ID,
                        type_: CLAP_EVENT_PARAM_VALUE,
                        flags: 0,
                    },
                    param_id: id,
                    cookie: std::ptr::null_mut(),
                    note_id: -1,
                    port_index: -1,
                    channel: -1,
                    key: -1,
                    value,
                });
            }
        }
        for e in events {
            let Some(m) = st.notes.midi(&e.event) else { continue };
            let time = (e.offset).min(n as u32 - 1);
            let mut push = |kind: u16, key: i16, velocity: f64| {
                if ev.notes.len() < MAX_EVENTS {
                    ev.notes.push(clap_event_note {
                        header: clap_event_header {
                            size: std::mem::size_of::<clap_event_note>() as u32,
                            time,
                            space_id: CLAP_CORE_EVENT_SPACE_ID,
                            type_: kind,
                            flags: 0,
                        },
                        note_id: -1,
                        port_index: 0,
                        channel: 0,
                        key,
                        velocity,
                    });
                }
            };
            match m[0] {
                0x90 => push(CLAP_EVENT_NOTE_ON, m[1] as i16, m[2] as f64 / 127.0),
                0x80 => push(CLAP_EVENT_NOTE_OFF, m[1] as i16, 0.5),
                // Wildcard key -1 releases every note.
                _ => push(CLAP_EVENT_NOTE_OFF, -1, 0.0),
            }
        }
        ev.order.extend(ev.params.iter().map(|e| &e.header as *const clap_event_header));
        ev.order.extend(ev.notes.iter().map(|e| &e.header as *const clap_event_header));
        let in_events =
            clap_input_events { ctx: ev as *mut InEvents as *mut c_void, size: Some(in_size), get: Some(in_get) };
        let out_events = clap_output_events { ctx: std::ptr::null_mut(), try_push: Some(out_try_push) };

        for (b, port) in st.in_bufs.iter_mut().enumerate() {
            for (c, buf) in port.iter_mut().enumerate() {
                match (b, c) {
                    (0, 0) => buf[..n].copy_from_slice(&left[..n]),
                    (0, 1) => buf[..n].copy_from_slice(&right[..n]),
                    _ => buf[..n].fill(0.0),
                }
                st.in_ptrs[b][c] = buf.as_mut_ptr();
            }
            st.in_ports[b].data32 = st.in_ptrs[b].as_mut_ptr();
        }
        for (b, port) in st.out_bufs.iter_mut().enumerate() {
            for (c, buf) in port.iter_mut().enumerate() {
                buf[..n].fill(0.0);
                st.out_ptrs[b][c] = buf.as_mut_ptr();
            }
            st.out_ports[b].data32 = st.out_ptrs[b].as_mut_ptr();
        }

        let t = transport::read();
        let beats_per_bar = t.ts_num.max(1) as f64 * 4.0 / t.ts_den.max(1) as f64;
        let bar = (t.ppq_pos / beats_per_bar).floor();
        st.transport.header = clap_event_header {
            size: std::mem::size_of::<clap_event_transport>() as u32,
            time: 0,
            space_id: CLAP_CORE_EVENT_SPACE_ID,
            type_: CLAP_EVENT_TRANSPORT,
            flags: 0,
        };
        st.transport.flags = CLAP_TRANSPORT_HAS_TEMPO
            | CLAP_TRANSPORT_HAS_BEATS_TIMELINE
            | CLAP_TRANSPORT_HAS_TIME_SIGNATURE
            | if t.playing { CLAP_TRANSPORT_IS_PLAYING } else { 0 };
        st.transport.song_pos_beats = (t.ppq_pos * CLAP_BEATTIME_FACTOR as f64) as i64;
        st.transport.tempo = t.bpm;
        st.transport.bar_start = (bar * beats_per_bar * CLAP_BEATTIME_FACTOR as f64) as i64;
        st.transport.bar_number = bar as i32;
        st.transport.tsig_num = t.ts_num as u16;
        st.transport.tsig_denom = t.ts_den as u16;

        let process = clap_process {
            steady_time: st.steady,
            frames_count: n as u32,
            transport: &st.transport,
            audio_inputs: st.in_ports.as_ptr(),
            audio_outputs: st.out_ports.as_mut_ptr(),
            audio_inputs_count: st.in_ports.len() as u32,
            audio_outputs_count: st.out_ports.len() as u32,
            in_events: &in_events,
            out_events: &out_events,
        };
        if let Some(f) = p.process {
            unsafe { f(self.plugin, &process) };
        }
        st.steady += n as i64;

        match st.out_bufs.first() {
            Some(port) if port.len() >= 2 => {
                left[..n].copy_from_slice(&port[0][..n]);
                right[..n].copy_from_slice(&port[1][..n]);
            }
            Some(port) if port.len() == 1 => {
                left[..n].copy_from_slice(&port[0][..n]);
                right[..n].copy_from_slice(&port[0][..n]);
            }
            _ => {
                left[..n].fill(0.0);
                right[..n].fill(0.0);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_plugin_api::NoteEvent;

    /// The test synth built by `cargo build --example test_clap_synth` (also built by `cargo test`).
    fn test_synth() -> Option<PathBuf> {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/examples/test_clap_synth.dll");
        p.is_file().then_some(p)
    }

    fn peak(p: &dyn PluginInstance, note: bool) -> f32 {
        let ev = [TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key: 69, velocity: 1.0 } }];
        let (mut l, mut r) = (vec![0.0f32; 4800], vec![0.0f32; 4800]);
        p.process(if note { &ev } else { &[] }, &mut l, &mut r);
        l.iter().fold(0.0f32, |m, x| m.max(x.abs()))
    }

    #[test]
    fn hosts_the_test_synth() {
        let Some(path) = test_synth() else {
            eprintln!("test_clap_synth.dll not built; skipping");
            return;
        };
        let d = describe(&path).unwrap().remove(0);
        assert!(d.is_instrument && d.name == "Soundesti Test Synth");
        let p = ClapPlugin::load(&d, 48_000.0).unwrap();
        assert_eq!(p.param_count(), 1);
        assert!((peak(&p, true) - 0.25).abs() < 0.01, "full sine x 0.5 gain x 0.5");
        // A UI parameter change reaches the processor on the next block.
        p.set_param_value(0, 1.0);
        assert!((peak(&p, false) - 0.5).abs() < 0.01);
        assert_eq!(p.param_display(0), "100%");
        let state = p.save_state().unwrap();
        p.set_param_value(0, 0.0);
        assert!(peak(&p, false) < 1e-6);
        assert!(p.load_state(&state));
        assert!((peak(&p, false) - 0.5).abs() < 0.01, "state restores the gain");
    }
}
