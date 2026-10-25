//! VST 2.4 hosting (independent implementation of the publicly documented C interface).
//! Needed above all for "FL Studio VSTi", which brings FL Studio's own instruments and
//! effects into other hosts.

use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use soundesti_plugin_api::{TimedEvent, transport};

use crate::{HostError, MAX_BLOCK, NoteMap, PluginDesc, PluginFormat, PluginInstance, editor};

type Dispatcher = unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
type ProcessFn = unsafe extern "C" fn(*mut AEffect, *const *const f32, *const *mut f32, i32);
type ProcessDoubleFn = unsafe extern "C" fn(*mut AEffect, *const *const f64, *const *mut f64, i32);
type SetParamFn = unsafe extern "C" fn(*mut AEffect, i32, f32);
type GetParamFn = unsafe extern "C" fn(*mut AEffect, i32) -> f32;
type HostCallback = unsafe extern "C" fn(*mut AEffect, i32, i32, isize, *mut c_void, f32) -> isize;
type MainFn = unsafe extern "C" fn(HostCallback) -> *mut AEffect;

const MAGIC: i32 = 0x5673_7450; // 'VstP'

#[repr(C)]
struct AEffect {
    magic: i32,
    dispatcher: Option<Dispatcher>,
    process: Option<ProcessFn>,
    set_parameter: Option<SetParamFn>,
    get_parameter: Option<GetParamFn>,
    num_programs: i32,
    num_params: i32,
    num_inputs: i32,
    num_outputs: i32,
    flags: i32,
    resvd1: isize,
    resvd2: isize,
    initial_delay: i32,
    real_qualities: i32,
    off_qualities: i32,
    io_ratio: f32,
    object: *mut c_void,
    user: *mut c_void,
    unique_id: i32,
    version: i32,
    process_replacing: Option<ProcessFn>,
    process_double_replacing: Option<ProcessDoubleFn>,
    future: [u8; 56],
}

// Plugin opcodes.
const EFF_OPEN: i32 = 0;
const EFF_CLOSE: i32 = 1;
const EFF_GET_PARAM_LABEL: i32 = 6;
const EFF_GET_PARAM_DISPLAY: i32 = 7;
const EFF_GET_PARAM_NAME: i32 = 8;
const EFF_SET_SAMPLE_RATE: i32 = 10;
const EFF_SET_BLOCK_SIZE: i32 = 11;
const EFF_MAINS_CHANGED: i32 = 12;
const EFF_EDIT_GET_RECT: i32 = 13;
const EFF_EDIT_OPEN: i32 = 14;
const EFF_EDIT_CLOSE: i32 = 15;
const EFF_EDIT_IDLE: i32 = 19;
const EFF_GET_CHUNK: i32 = 23;
const EFF_SET_CHUNK: i32 = 24;
const EFF_PROCESS_EVENTS: i32 = 25;
const EFF_GET_EFFECT_NAME: i32 = 45;
const EFF_GET_VENDOR_STRING: i32 = 47;
const EFF_GET_PRODUCT_STRING: i32 = 48;
const EFF_START_PROCESS: i32 = 71;
const EFF_STOP_PROCESS: i32 = 72;

const FLAG_HAS_EDITOR: i32 = 1;
const FLAG_CAN_REPLACING: i32 = 1 << 4;
const FLAG_PROGRAM_CHUNKS: i32 = 1 << 5;
const FLAG_IS_SYNTH: i32 = 1 << 8;

const MAX_EVENTS: usize = 512;

#[repr(C)]
#[derive(Clone, Copy)]
struct VstMidiEvent {
    kind: i32,
    byte_size: i32,
    delta_frames: i32,
    flags: i32,
    note_length: i32,
    note_offset: i32,
    midi_data: [u8; 4],
    detune: i8,
    note_off_velocity: u8,
    reserved1: u8,
    reserved2: u8,
}

#[repr(C)]
struct VstEvents {
    num_events: i32,
    reserved: isize,
    events: [*mut VstMidiEvent; MAX_EVENTS],
}

#[repr(C)]
#[derive(Default)]
struct VstTimeInfo {
    sample_pos: f64,
    sample_rate: f64,
    nano_seconds: f64,
    ppq_pos: f64,
    tempo: f64,
    bar_start_pos: f64,
    cycle_start_pos: f64,
    cycle_end_pos: f64,
    time_sig_numerator: i32,
    time_sig_denominator: i32,
    smpte_offset: i32,
    smpte_frame_rate: i32,
    samples_to_next_clock: i32,
    flags: i32,
}

#[repr(C)]
struct ERect {
    top: i16,
    left: i16,
    bottom: i16,
    right: i16,
}

thread_local! {
    static TIME_INFO: UnsafeCell<VstTimeInfo> = UnsafeCell::new(VstTimeInfo::default());
}

/// Called by plugins. May run on any thread.
unsafe extern "C" fn host_callback(
    effect: *mut AEffect,
    opcode: i32,
    index: i32,
    value: isize,
    ptr: *mut c_void,
    _opt: f32,
) -> isize {
    if debug_log() {
        eprintln!("[vst2 host] t={:?} opcode {opcode} index {index} value {value}", std::thread::current().id());
    }
    match opcode {
        1 => 2400,            // audioMasterVersion
        6 => 1,               // audioMasterWantMidi (legacy)
        7 => time_info_ptr(), // audioMasterGetTime
        13 => 1,              // audioMasterIOChanged
        15 => {
            // audioMasterSizeWindow(width = index, height = value)
            editor::resize_for(effect as usize, index, value as i32);
            1
        }
        16 => transport::read().sample_rate as isize,
        17 => MAX_BLOCK as isize,
        23 => 0, // audioMasterGetCurrentProcessLevel: unknown
        24 => 1, // audioMasterGetAutomationState: off
        32 | 33 => {
            // Vendor / product string (buffer of at least 64 bytes).
            if !ptr.is_null() {
                let name = b"Soundesti\0";
                unsafe { std::ptr::copy_nonoverlapping(name.as_ptr(), ptr as *mut u8, name.len()) };
            }
            1
        }
        34 => 400, // vendor version
        37 => {
            // audioMasterCanDo
            let what = if ptr.is_null() {
                ""
            } else {
                unsafe { std::ffi::CStr::from_ptr(ptr as *const _) }.to_str().unwrap_or("")
            };
            matches!(
                what,
                "sendVstEvents"
                    | "sendVstMidiEvent"
                    | "sendVstTimeInfo"
                    | "receiveVstEvents"
                    | "receiveVstMidiEvent"
                    | "sizeWindow"
                    | "startStopProcess"
                    | "supplyIdle"
            ) as isize
        }
        38 => 1, // English
        42 => 1, // audioMasterUpdateDisplay
        _ => 0,
    }
}

/// Set SOUNDESTI_VST_LOG=1 to print every plugin-to-host call (debugging aid).
fn debug_log() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SOUNDESTI_VST_LOG").is_some())
}

fn time_info_ptr() -> isize {
    let t = transport::read();
    TIME_INFO.with(|cell| {
        let ti = unsafe { &mut *cell.get() };
        let beats_per_bar = t.ts_num.max(1) as f64 * 4.0 / t.ts_den.max(1) as f64;
        *ti = VstTimeInfo {
            sample_pos: t.sample_pos as f64,
            sample_rate: t.sample_rate,
            ppq_pos: t.ppq_pos,
            tempo: t.bpm,
            bar_start_pos: (t.ppq_pos / beats_per_bar).floor() * beats_per_bar,
            time_sig_numerator: t.ts_num as i32,
            time_sig_denominator: t.ts_den as i32,
            // playing | ppq valid | tempo valid | bars valid | time signature valid
            flags: (if t.playing { 2 } else { 0 }) | 1 << 9 | 1 << 10 | 1 << 11 | 1 << 13,
            ..Default::default()
        };
        cell.get() as isize
    })
}

struct AudioState {
    notes: NoteMap,
    midi: Box<[VstMidiEvent; MAX_EVENTS]>,
    events: Box<VstEvents>,
    ins: Vec<Vec<f32>>,
    outs: Vec<Vec<f32>>,
    in_ptrs: Vec<*const f32>,
    out_ptrs: Vec<*mut f32>,
}

pub struct Vst2Plugin {
    desc: PluginDesc,
    effect: *mut AEffect,
    audio: UnsafeCell<AudioState>,
    editor_open: AtomicBool,
    // Never unloaded: plugins (FL Studio VSTi among them) keep background threads running
    // after effClose, and unloading their code under them crashes the process.
    _lib: std::mem::ManuallyDrop<libloading::Library>,
}

// The VST2 interface allows the UI-thread calls (editor, parameters, chunks) to run
// alongside processReplacing on the audio thread; `audio` is only touched by `process`.
unsafe impl Send for Vst2Plugin {}
unsafe impl Sync for Vst2Plugin {}

fn open(path: &Path) -> Result<(libloading::Library, *mut AEffect), HostError> {
    let lib = unsafe { libloading::Library::new(path) }.map_err(|e| HostError::Load(e.to_string()))?;
    let main: MainFn = unsafe {
        match lib.get::<MainFn>(b"VSTPluginMain\0") {
            Ok(f) => *f,
            Err(_) => *lib.get::<MainFn>(b"main\0").map_err(|_| HostError::NotAPlugin("no VST entry point".into()))?,
        }
    };
    let effect = unsafe { main(host_callback) };
    if effect.is_null() || unsafe { (*effect).magic } != MAGIC {
        return Err(HostError::NotAPlugin("the VST entry point returned no plugin".into()));
    }
    if unsafe { (*effect).dispatcher.is_none() } {
        return Err(HostError::NotAPlugin("plugin has no dispatcher".into()));
    }
    Ok((lib, effect))
}

unsafe fn dispatch(effect: *mut AEffect, opcode: i32, index: i32, value: isize, ptr: *mut c_void, opt: f32) -> isize {
    match unsafe { (*effect).dispatcher } {
        Some(d) => unsafe { d(effect, opcode, index, value, ptr, opt) },
        None => 0,
    }
}

unsafe fn string(effect: *mut AEffect, opcode: i32, index: i32) -> String {
    let mut buf = [0u8; 256];
    unsafe { dispatch(effect, opcode, index, 0, buf.as_mut_ptr() as *mut c_void, 0.0) };
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    String::from_utf8_lossy(&buf[..end]).trim().to_string()
}

/// Loads a plugin just long enough to read its name, vendor and type.
pub fn describe(path: &Path) -> Result<PluginDesc, HostError> {
    let (lib, effect) = open(path)?;
    let desc = unsafe {
        // Ask for the name before effOpen: most plugins answer, and big ones (FL Studio
        // VSTi) do their heavy start-up, including pop-ups, only in effOpen.
        let mut name = string(effect, EFF_GET_EFFECT_NAME, 0);
        if name.is_empty() {
            name = string(effect, EFF_GET_PRODUCT_STRING, 0);
        }
        if name.is_empty() {
            dispatch(effect, EFF_OPEN, 0, 0, std::ptr::null_mut(), 0.0);
            name = string(effect, EFF_GET_EFFECT_NAME, 0);
        }
        if name.is_empty() {
            name = string(effect, EFF_GET_PRODUCT_STRING, 0);
        }
        if name.is_empty() {
            name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        }
        let e = &*effect;
        let d = PluginDesc {
            format: PluginFormat::Vst2,
            path: path.to_path_buf(),
            id: format!("{:08X}", e.unique_id as u32),
            name,
            vendor: string(effect, EFF_GET_VENDOR_STRING, 0),
            is_instrument: e.flags & FLAG_IS_SYNTH != 0,
            inputs: e.num_inputs.max(0) as u32,
            outputs: e.num_outputs.max(0) as u32,
        };
        // effClose also frees the plugin object, so it is needed even without effOpen.
        dispatch(effect, EFF_CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
        d
    };
    drop(lib);
    Ok(desc)
}

impl Vst2Plugin {
    pub fn load(desc: &PluginDesc, sample_rate: f32) -> Result<Self, HostError> {
        let (lib, effect) = open(&desc.path)?;
        let e = unsafe { &*effect };
        if e.flags & FLAG_CAN_REPLACING == 0 || e.process_replacing.is_none() {
            unsafe { dispatch(effect, EFF_CLOSE, 0, 0, std::ptr::null_mut(), 0.0) };
            return Err(HostError::NotAPlugin("plugin does not support replacing processing (too old)".into()));
        }
        let (ni, no) = (e.num_inputs.clamp(0, 64) as usize, e.num_outputs.clamp(0, 64) as usize);
        unsafe {
            dispatch(effect, EFF_OPEN, 0, 0, std::ptr::null_mut(), 0.0);
            dispatch(effect, EFF_SET_SAMPLE_RATE, 0, 0, std::ptr::null_mut(), sample_rate);
            dispatch(effect, EFF_SET_BLOCK_SIZE, 0, MAX_BLOCK as isize, std::ptr::null_mut(), 0.0);
            dispatch(effect, EFF_MAINS_CHANGED, 0, 1, std::ptr::null_mut(), 0.0);
            dispatch(effect, EFF_START_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
        }
        let zero_midi = VstMidiEvent {
            kind: 1,
            byte_size: 32,
            delta_frames: 0,
            flags: 1,
            note_length: 0,
            note_offset: 0,
            midi_data: [0; 4],
            detune: 0,
            note_off_velocity: 0,
            reserved1: 0,
            reserved2: 0,
        };
        let audio = AudioState {
            notes: NoteMap::new(),
            midi: Box::new([zero_midi; MAX_EVENTS]),
            events: Box::new(VstEvents { num_events: 0, reserved: 0, events: [std::ptr::null_mut(); MAX_EVENTS] }),
            ins: (0..ni).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            outs: (0..no).map(|_| vec![0.0; MAX_BLOCK]).collect(),
            in_ptrs: vec![std::ptr::null(); ni],
            out_ptrs: vec![std::ptr::null_mut(); no],
        };
        let mut desc = desc.clone();
        desc.inputs = ni as u32;
        desc.outputs = no as u32;
        Ok(Self {
            desc,
            effect,
            audio: UnsafeCell::new(audio),
            editor_open: AtomicBool::new(false),
            _lib: std::mem::ManuallyDrop::new(lib),
        })
    }

    fn flags(&self) -> i32 {
        unsafe { (*self.effect).flags }
    }
}

impl Drop for Vst2Plugin {
    fn drop(&mut self) {
        // DLLs the plugin loaded since (e.g. when FL Studio VSTi opened FL) stay loaded.
        crate::pin::pin_new_modules();
        unsafe {
            if self.editor_open.swap(false, Ordering::AcqRel) {
                dispatch(self.effect, EFF_EDIT_CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
            }
            dispatch(self.effect, EFF_STOP_PROCESS, 0, 0, std::ptr::null_mut(), 0.0);
            dispatch(self.effect, EFF_MAINS_CHANGED, 0, 0, std::ptr::null_mut(), 0.0);
            dispatch(self.effect, EFF_CLOSE, 0, 0, std::ptr::null_mut(), 0.0);
        }
    }
}

/// Fallback state for plugins without chunks: every parameter value.
const PARAM_STATE_MAGIC: &[u8; 4] = b"SDP2";

impl PluginInstance for Vst2Plugin {
    fn desc(&self) -> &PluginDesc {
        &self.desc
    }

    fn param_count(&self) -> usize {
        unsafe { (*self.effect).num_params.max(0) as usize }
    }

    fn param_name(&self, index: usize) -> String {
        unsafe { string(self.effect, EFF_GET_PARAM_NAME, index as i32) }
    }

    fn param_value(&self, index: usize) -> f32 {
        match unsafe { (*self.effect).get_parameter } {
            Some(f) if index < self.param_count() => unsafe { f(self.effect, index as i32) },
            _ => 0.0,
        }
    }

    fn param_display(&self, index: usize) -> String {
        let value = unsafe { string(self.effect, EFF_GET_PARAM_DISPLAY, index as i32) };
        let label = unsafe { string(self.effect, EFF_GET_PARAM_LABEL, index as i32) };
        if label.is_empty() { value } else { format!("{value} {label}") }
    }

    fn set_param_value(&self, index: usize, value: f32) {
        if let Some(f) = unsafe { (*self.effect).set_parameter }
            && index < self.param_count()
        {
            unsafe { f(self.effect, index as i32, value.clamp(0.0, 1.0)) };
        }
    }

    fn save_state(&self) -> Option<Vec<u8>> {
        if self.flags() & FLAG_PROGRAM_CHUNKS != 0 {
            let mut data: *mut u8 = std::ptr::null_mut();
            let len =
                unsafe { dispatch(self.effect, EFF_GET_CHUNK, 0, 0, &mut data as *mut *mut u8 as *mut c_void, 0.0) };
            if len > 0 && !data.is_null() {
                return Some(unsafe { std::slice::from_raw_parts(data, len as usize) }.to_vec());
            }
        }
        let mut out = PARAM_STATE_MAGIC.to_vec();
        for i in 0..self.param_count() {
            out.extend_from_slice(&self.param_value(i).to_le_bytes());
        }
        Some(out)
    }

    fn load_state(&self, data: &[u8]) -> bool {
        if let Some(values) = data.strip_prefix(PARAM_STATE_MAGIC) {
            for (i, v) in values.as_chunks::<4>().0.iter().enumerate().take(self.param_count()) {
                self.set_param_value(i, f32::from_le_bytes(*v));
            }
            return true;
        }
        if self.flags() & FLAG_PROGRAM_CHUNKS == 0 || data.is_empty() {
            return false;
        }
        let mut copy = data.to_vec();
        unsafe { dispatch(self.effect, EFF_SET_CHUNK, 0, copy.len() as isize, copy.as_mut_ptr() as *mut c_void, 0.0) };
        true
    }

    fn has_editor(&self) -> bool {
        self.flags() & FLAG_HAS_EDITOR != 0
    }

    fn open_editor(&self, parent: isize) -> Option<(i32, i32)> {
        if !self.has_editor() {
            return None;
        }
        editor::register_resize_target(self.effect as usize, parent);
        unsafe { dispatch(self.effect, EFF_EDIT_OPEN, 0, 0, parent as *mut c_void, 0.0) };
        self.editor_open.store(true, Ordering::Release);
        let mut rect: *mut ERect = std::ptr::null_mut();
        unsafe { dispatch(self.effect, EFF_EDIT_GET_RECT, 0, 0, &mut rect as *mut *mut ERect as *mut c_void, 0.0) };
        if rect.is_null() {
            return Some((640, 480));
        }
        let r = unsafe { &*rect };
        Some(((r.right - r.left).max(100) as i32, (r.bottom - r.top).max(60) as i32))
    }

    fn close_editor(&self) {
        crate::pin::pin_new_modules();
        if self.editor_open.swap(false, Ordering::AcqRel) {
            unsafe { dispatch(self.effect, EFF_EDIT_CLOSE, 0, 0, std::ptr::null_mut(), 0.0) };
        }
        editor::unregister_resize_target(self.effect as usize);
    }

    fn editor_idle(&self) {
        if self.editor_open.load(Ordering::Acquire) {
            unsafe { dispatch(self.effect, EFF_EDIT_IDLE, 0, 0, std::ptr::null_mut(), 0.0) };
        }
    }

    fn process(&self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]) {
        let st = unsafe { &mut *self.audio.get() };
        let n = left.len().min(right.len()).min(MAX_BLOCK);
        let mut count = 0;
        for e in events {
            if count == MAX_EVENTS {
                break;
            }
            if let Some(m) = st.notes.midi(&e.event) {
                let ev = &mut st.midi[count];
                ev.delta_frames = (e.offset as i32).min(n as i32 - 1).max(0);
                ev.midi_data = [m[0], m[1], m[2], 0];
                count += 1;
            }
        }
        if count > 0 {
            st.events.num_events = count as i32;
            for i in 0..count {
                st.events.events[i] = &mut st.midi[i];
            }
            unsafe {
                dispatch(self.effect, EFF_PROCESS_EVENTS, 0, 0, &mut *st.events as *mut VstEvents as *mut c_void, 0.0)
            };
        }
        for (i, buf) in st.ins.iter_mut().enumerate() {
            match i {
                0 => buf[..n].copy_from_slice(&left[..n]),
                1 => buf[..n].copy_from_slice(&right[..n]),
                _ => buf[..n].fill(0.0),
            }
            st.in_ptrs[i] = buf.as_ptr();
        }
        for (i, buf) in st.outs.iter_mut().enumerate() {
            buf[..n].fill(0.0);
            st.out_ptrs[i] = buf.as_mut_ptr();
        }
        if let Some(f) = unsafe { (*self.effect).process_replacing } {
            unsafe { f(self.effect, st.in_ptrs.as_ptr(), st.out_ptrs.as_ptr(), n as i32) };
        }
        match st.outs.len() {
            0 => {
                left[..n].fill(0.0);
                right[..n].fill(0.0);
            }
            1 => {
                left[..n].copy_from_slice(&st.outs[0][..n]);
                right[..n].copy_from_slice(&st.outs[0][..n]);
            }
            _ => {
                left[..n].copy_from_slice(&st.outs[0][..n]);
                right[..n].copy_from_slice(&st.outs[1][..n]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abi_sizes_match_the_vst2_layout() {
        // 64-bit Windows layout of the public VST 2.4 structs.
        assert_eq!(std::mem::size_of::<VstMidiEvent>(), 32);
        assert_eq!(std::mem::offset_of!(AEffect, num_programs), 40);
        assert_eq!(std::mem::offset_of!(AEffect, unique_id), 112);
        assert_eq!(std::mem::offset_of!(AEffect, process_replacing), 120);
        assert_eq!(std::mem::offset_of!(VstTimeInfo, time_sig_numerator), 64);
        assert_eq!(std::mem::offset_of!(VstEvents, events), 16);
    }

    #[test]
    fn host_answers_basic_queries() {
        unsafe {
            assert_eq!(host_callback(std::ptr::null_mut(), 1, 0, 0, std::ptr::null_mut(), 0.0), 2400);
            let ti = host_callback(std::ptr::null_mut(), 7, 0, 0, std::ptr::null_mut(), 0.0) as *const VstTimeInfo;
            assert!(!ti.is_null() && (*ti).tempo > 0.0);
            let can = c"sendVstTimeInfo";
            assert_eq!(host_callback(std::ptr::null_mut(), 37, 0, 0, can.as_ptr() as *mut c_void, 0.0), 1);
        }
        assert!(matches!(describe(Path::new("C:\\definitely\\missing.dll")), Err(HostError::Load(_))));
    }
}
