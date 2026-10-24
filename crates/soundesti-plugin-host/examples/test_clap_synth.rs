//! A minimal CLAP instrument used to test Soundesti's CLAP host (no CLAP plugins ship
//! with Windows). One sine voice per held key, one "Gain" parameter, state = the gain.
//! Build: `cargo build -p soundesti-plugin-host --example test_clap_synth`
//! (produces `target/debug/examples/test_clap_synth.dll`; rename to `.clap` to scan it).

#![allow(clippy::missing_safety_doc)]

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};

use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::params::*;
use clap_sys::ext::state::*;
use clap_sys::factory::plugin_factory::*;
use clap_sys::host::clap_host;
use clap_sys::plugin::*;
use clap_sys::process::*;
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::CLAP_VERSION;

const ID: &CStr = c"soundesti.test-synth";
const GAIN: u32 = 1;

struct Synth {
    plugin: clap_plugin,
    gain: AtomicU64,
    keys: [bool; 128],
    phase: [f64; 128],
    sr: f64,
}

static FEATURES: [usize; 2] = [0, 0];

fn features() -> *const *const c_char {
    static LIST: [Option<&CStr>; 2] = [Some(c"instrument"), None];
    let _ = FEATURES;
    // A null-terminated array of C strings.
    static mut PTRS: [*const c_char; 2] = [std::ptr::null(); 2];
    unsafe {
        PTRS[0] = LIST[0].unwrap().as_ptr();
        std::ptr::addr_of!(PTRS) as *const *const c_char
    }
}

static mut DESC: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: std::ptr::null(),
    name: std::ptr::null(),
    vendor: std::ptr::null(),
    url: std::ptr::null(),
    manual_url: std::ptr::null(),
    support_url: std::ptr::null(),
    version: std::ptr::null(),
    description: std::ptr::null(),
    features: std::ptr::null(),
};

fn desc() -> *const clap_plugin_descriptor {
    unsafe {
        DESC.id = ID.as_ptr();
        DESC.name = c"Soundesti Test Synth".as_ptr();
        DESC.vendor = c"Soundesti".as_ptr();
        DESC.url = c"".as_ptr();
        DESC.manual_url = c"".as_ptr();
        DESC.support_url = c"".as_ptr();
        DESC.version = c"1.0".as_ptr();
        DESC.description = c"Test instrument".as_ptr();
        DESC.features = features();
        std::ptr::addr_of!(DESC)
    }
}

unsafe fn synth<'a>(p: *const clap_plugin) -> &'a mut Synth {
    unsafe { &mut *((*p).plugin_data as *mut Synth) }
}

unsafe extern "C" fn init(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    drop(unsafe { Box::from_raw((*p).plugin_data as *mut Synth) });
}
unsafe extern "C" fn activate(p: *const clap_plugin, sr: f64, _min: u32, _max: u32) -> bool {
    unsafe { synth(p).sr = sr };
    true
}
unsafe extern "C" fn deactivate(_p: *const clap_plugin) {}
unsafe extern "C" fn start(_p: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn stop(_p: *const clap_plugin) {}
unsafe extern "C" fn reset(_p: *const clap_plugin) {}

unsafe extern "C" fn process(p: *const clap_plugin, pr: *const clap_process) -> clap_process_status {
    let s = unsafe { synth(p) };
    let pr = unsafe { &*pr };
    let ev = unsafe { &*pr.in_events };
    let count = unsafe { ev.size.unwrap()(ev) };
    for i in 0..count {
        let h = unsafe { &*ev.get.unwrap()(ev, i) };
        match h.type_ {
            CLAP_EVENT_NOTE_ON => {
                let n = unsafe { &*(h as *const _ as *const clap_event_note) };
                if (0..128).contains(&n.key) {
                    s.keys[n.key as usize] = true;
                }
            }
            CLAP_EVENT_NOTE_OFF => {
                let n = unsafe { &*(h as *const _ as *const clap_event_note) };
                if n.key < 0 {
                    s.keys = [false; 128];
                } else if n.key < 128 {
                    s.keys[n.key as usize] = false;
                }
            }
            CLAP_EVENT_PARAM_VALUE => {
                let v = unsafe { &*(h as *const _ as *const clap_event_param_value) };
                if v.param_id == GAIN {
                    s.gain.store(v.value.to_bits(), Ordering::Relaxed);
                }
            }
            _ => {}
        }
    }
    let gain = f64::from_bits(s.gain.load(Ordering::Relaxed));
    let out = unsafe { &*pr.audio_outputs };
    let chans = unsafe { std::slice::from_raw_parts(out.data32, out.channel_count as usize) };
    for f in 0..pr.frames_count as usize {
        let mut v = 0.0;
        for k in 0..128 {
            if s.keys[k] {
                let freq = 440.0 * 2f64.powf((k as f64 - 69.0) / 12.0);
                v += (s.phase[k] * std::f64::consts::TAU).sin();
                s.phase[k] = (s.phase[k] + freq / s.sr).fract();
            }
        }
        for c in chans {
            unsafe { *c.add(f) = (v * gain * 0.5) as f32 };
        }
    }
    CLAP_PROCESS_CONTINUE
}

unsafe extern "C" fn params_count(_p: *const clap_plugin) -> u32 {
    1
}
unsafe extern "C" fn params_info(_p: *const clap_plugin, index: u32, info: *mut clap_param_info) -> bool {
    if index != 0 {
        return false;
    }
    let info = unsafe { &mut *info };
    info.id = GAIN;
    info.flags = CLAP_PARAM_IS_AUTOMATABLE;
    info.cookie = std::ptr::null_mut();
    for (d, s) in info.name.iter_mut().zip(b"Gain\0") {
        *d = *s as c_char;
    }
    info.module[0] = 0;
    info.min_value = 0.0;
    info.max_value = 1.0;
    info.default_value = 0.5;
    true
}
unsafe extern "C" fn params_value(p: *const clap_plugin, id: u32, out: *mut f64) -> bool {
    if id != GAIN {
        return false;
    }
    unsafe { *out = f64::from_bits(synth(p).gain.load(Ordering::Relaxed)) };
    true
}
unsafe extern "C" fn params_to_text(_p: *const clap_plugin, _id: u32, value: f64, buf: *mut c_char, cap: u32) -> bool {
    let s = format!("{:.0}%\0", value * 100.0);
    for (i, b) in s.bytes().take(cap as usize).enumerate() {
        unsafe { *buf.add(i) = b as c_char };
    }
    true
}
unsafe extern "C" fn params_from_text(_p: *const clap_plugin, _id: u32, _t: *const c_char, _out: *mut f64) -> bool {
    false
}
unsafe extern "C" fn params_flush(
    _p: *const clap_plugin,
    _in: *const clap_input_events,
    _out: *const clap_output_events,
) {
}

static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(params_count),
    get_info: Some(params_info),
    get_value: Some(params_value),
    value_to_text: Some(params_to_text),
    text_to_value: Some(params_from_text),
    flush: Some(params_flush),
};

unsafe extern "C" fn state_save(p: *const clap_plugin, s: *const clap_ostream) -> bool {
    let bytes = unsafe { synth(p).gain.load(Ordering::Relaxed) }.to_le_bytes();
    unsafe { (*s).write.unwrap()(s, bytes.as_ptr() as *const c_void, 8) == 8 }
}
unsafe extern "C" fn state_load(p: *const clap_plugin, s: *const clap_istream) -> bool {
    let mut bytes = [0u8; 8];
    if unsafe { (*s).read.unwrap()(s, bytes.as_mut_ptr() as *mut c_void, 8) } != 8 {
        return false;
    }
    unsafe { synth(p).gain.store(u64::from_le_bytes(bytes), Ordering::Relaxed) };
    true
}

static STATE: clap_plugin_state = clap_plugin_state { save: Some(state_save), load: Some(state_load) };

unsafe extern "C" fn get_extension(_p: *const clap_plugin, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_PARAMS {
        &PARAMS as *const _ as *const c_void
    } else if id == CLAP_EXT_STATE {
        &STATE as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}
unsafe extern "C" fn on_main_thread(_p: *const clap_plugin) {}

unsafe extern "C" fn factory_count(_f: *const clap_plugin_factory) -> u32 {
    1
}
unsafe extern "C" fn factory_desc(_f: *const clap_plugin_factory, index: u32) -> *const clap_plugin_descriptor {
    if index == 0 { desc() } else { std::ptr::null() }
}
unsafe extern "C" fn factory_create(
    _f: *const clap_plugin_factory,
    _host: *const clap_host,
    id: *const c_char,
) -> *const clap_plugin {
    if unsafe { CStr::from_ptr(id) } != ID {
        return std::ptr::null();
    }
    let s = Box::into_raw(Box::new(Synth {
        plugin: clap_plugin {
            desc: desc(),
            plugin_data: std::ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(start),
            stop_processing: Some(stop),
            reset: Some(reset),
            process: Some(process),
            get_extension: Some(get_extension),
            on_main_thread: Some(on_main_thread),
        },
        gain: AtomicU64::new(0.5f64.to_bits()),
        keys: [false; 128],
        phase: [0.0; 128],
        sr: 48_000.0,
    }));
    unsafe {
        (*s).plugin.plugin_data = s as *mut c_void;
        &(*s).plugin
    }
}

static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(factory_count),
    get_plugin_descriptor: Some(factory_desc),
    create_plugin: Some(factory_create),
};

unsafe extern "C" fn entry_init(_path: *const c_char) -> bool {
    true
}
unsafe extern "C" fn entry_deinit() {}
unsafe extern "C" fn entry_factory(id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID {
        &FACTORY as *const _ as *const c_void
    } else {
        std::ptr::null()
    }
}

#[unsafe(no_mangle)]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(entry_deinit),
    get_factory: Some(entry_factory),
};
