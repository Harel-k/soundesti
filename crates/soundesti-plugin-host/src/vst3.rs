//! VST3 hosting through the `vst3` crate's COM bindings (MIT/Apache).
//!
//! A VST3 plugin is a processor (`IComponent` + `IAudioProcessor`, audio thread) and an
//! edit controller (`IEditController`, UI thread), sometimes the same object. The host
//! supplies COM objects of its own: the host context, event lists, parameter-change
//! queues, memory streams for state, a component handler for edits made in the plugin's
//! window, and a frame for editor resizes.

#![allow(non_snake_case)]

use std::cell::UnsafeCell;
use std::collections::HashMap;
use std::ffi::{CStr, c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use soundesti_plugin_api::{TimedEvent, transport};
use vst3::Steinberg::Vst::*;
use vst3::Steinberg::*;
use vst3::{Class, ComPtr, ComWrapper, Interface};

use crate::{HostError, MAX_BLOCK, NoteMap, PluginDesc, PluginFormat, PluginInstance, editor};

type GetFactoryFn = unsafe extern "system" fn() -> *mut IPluginFactory;
type InitDllFn = unsafe extern "system" fn() -> bool;

const MAX_EVENTS: usize = 512;
const MAX_PARAM_QUEUES: usize = 64;
const AUDIO_CLASS: &str = "Audio Module Class";

fn cstr(buf: &[c_char]) -> String {
    let bytes: Vec<u8> = buf.iter().take_while(|c| **c != 0).map(|c| *c as u8).collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn wstr(buf: &[TChar]) -> String {
    let units: Vec<u16> = buf.iter().take_while(|c| **c != 0).copied().collect();
    String::from_utf16_lossy(&units).trim().to_string()
}

fn copy_wstr(src: &str, dst: &mut [TChar]) {
    let mut n = 0;
    for (d, s) in dst.iter_mut().zip(src.encode_utf16()) {
        *d = s as TChar;
        n += 1;
    }
    if let Some(end) = dst.get_mut(n.min(dst.len().saturating_sub(1))) {
        *end = 0;
    }
}

fn tuid_hex(t: &TUID) -> String {
    t.iter().map(|b| format!("{:02X}", *b as u8)).collect()
}

fn tuid_from_hex(s: &str) -> Option<TUID> {
    if s.len() != 32 {
        return None;
    }
    let mut t: TUID = [0; 16];
    for (i, v) in t.iter_mut().enumerate() {
        *v = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()? as c_char;
    }
    Some(t)
}

/// The DLL inside a `.vst3` bundle folder, or the file itself.
fn binary_path(path: &Path) -> PathBuf {
    if path.is_dir() {
        let stem = path.file_stem().map(|s| s.to_os_string()).unwrap_or_default();
        let mut file = stem;
        file.push(".vst3");
        let dir = path.join("Contents").join("x86_64-win");
        let dll = dir.join(&file);
        if dll.is_file() {
            return dll;
        }
        // Some bundles name the binary differently (e.g. with a version number).
        if let Some(found) = std::fs::read_dir(&dir).ok().and_then(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .find(|p| p.is_file() && p.extension().is_some_and(|x| x.eq_ignore_ascii_case("vst3")))
        }) {
            return found;
        }
    }
    path.to_path_buf()
}

struct Module {
    _lib: std::mem::ManuallyDrop<libloading::Library>,
    factory: ComPtr<IPluginFactory>,
}

fn open(path: &Path) -> Result<Module, HostError> {
    let lib = unsafe { libloading::Library::new(binary_path(path)) }.map_err(|e| HostError::Load(e.to_string()))?;
    unsafe {
        if let Ok(init) = lib.get::<InitDllFn>(b"InitDll\0") {
            init();
        }
        let get: GetFactoryFn = *lib
            .get::<GetFactoryFn>(b"GetPluginFactory\0")
            .map_err(|_| HostError::NotAPlugin("no VST3 entry point".into()))?;
        let factory =
            ComPtr::from_raw(get()).ok_or_else(|| HostError::NotAPlugin("the plugin returned no factory".into()))?;
        // Never unloaded (see vst2.rs: plugins keep threads alive after shutdown).
        Ok(Module { _lib: std::mem::ManuallyDrop::new(lib), factory })
    }
}

/// Every audio processor class in a .vst3 file.
pub fn describe(path: &Path) -> Result<Vec<PluginDesc>, HostError> {
    let m = open(path)?;
    let mut out = Vec::new();
    unsafe {
        let mut finfo: PFactoryInfo = std::mem::zeroed();
        let factory_vendor =
            if m.factory.getFactoryInfo(&mut finfo) == kResultOk { cstr(&finfo.vendor) } else { String::new() };
        let f2 = m.factory.cast::<IPluginFactory2>();
        for i in 0..m.factory.countClasses() {
            let mut info: PClassInfo = std::mem::zeroed();
            if m.factory.getClassInfo(i, &mut info) != kResultOk || cstr(&info.category) != AUDIO_CLASS {
                continue;
            }
            let (mut vendor, mut sub) = (factory_vendor.clone(), String::new());
            if let Some(f2) = &f2 {
                let mut i2: PClassInfo2 = std::mem::zeroed();
                if f2.getClassInfo2(i, &mut i2) == kResultOk {
                    sub = cstr(&i2.subCategories);
                    let v = cstr(&i2.vendor);
                    if !v.is_empty() {
                        vendor = v;
                    }
                }
            }
            out.push(PluginDesc {
                format: PluginFormat::Vst3,
                path: path.to_path_buf(),
                id: tuid_hex(&info.cid),
                name: cstr(&info.name),
                vendor,
                // Sub-categories look like "Instrument|Synth" or "Fx|EQ".
                is_instrument: sub.split('|').any(|s| s.eq_ignore_ascii_case("Instrument")),
                inputs: 0,
                outputs: 0,
            });
        }
    }
    // The library stays loaded (the scanner process exits right after).
    if out.is_empty() {
        return Err(HostError::NotAPlugin("no audio processor in this file".into()));
    }
    Ok(out)
}

// ----- host-side COM objects ------------------------------------------------

struct HostApp;

impl Class for HostApp {
    type Interfaces = (IHostApplication,);
}

impl IHostApplicationTrait for HostApp {
    unsafe fn getName(&self, name: *mut String128) -> tresult {
        unsafe { copy_wstr("Soundesti", &mut *name) };
        kResultOk
    }

    unsafe fn createInstance(&self, cid: *mut TUID, iid: *mut TUID, obj: *mut *mut c_void) -> tresult {
        // Plugins ask the host for message objects to talk between processor and controller.
        let (cid, iid) = unsafe { (*cid, *iid) };
        let same = |t: &TUID, g: &vst3::com_scrape_types::Guid| t.iter().zip(g).all(|(a, b)| *a as u8 == *b);
        let want = |g: &vst3::com_scrape_types::Guid| same(&cid, g) || same(&iid, g);
        let created: Option<ComPtr<FUnknown>> = if want(&IMessage::IID) {
            ComWrapper::new(HostMessage::default()).to_com_ptr::<FUnknown>()
        } else if want(&IAttributeList::IID) {
            ComWrapper::new(AttrList::default()).to_com_ptr::<FUnknown>()
        } else {
            None
        };
        match created {
            Some(p) => unsafe { ((*(*p.as_ptr()).vtbl).queryInterface)(p.as_ptr(), &iid, obj) },
            None => {
                unsafe { *obj = std::ptr::null_mut() };
                kResultFalse
            }
        }
    }
}

#[derive(Default)]
struct HostMessage {
    id: Mutex<Option<std::ffi::CString>>,
    attrs: Mutex<Option<ComPtr<IAttributeList>>>,
}

impl Class for HostMessage {
    type Interfaces = (IMessage,);
}

impl IMessageTrait for HostMessage {
    unsafe fn getMessageID(&self) -> FIDString {
        self.id.lock().ok().and_then(|g| g.as_ref().map(|s| s.as_ptr())).unwrap_or(std::ptr::null())
    }

    unsafe fn setMessageID(&self, id: FIDString) {
        if let Ok(mut g) = self.id.lock() {
            *g = (!id.is_null()).then(|| unsafe { CStr::from_ptr(id) }.to_owned());
        }
    }

    unsafe fn getAttributes(&self) -> *mut IAttributeList {
        let Ok(mut g) = self.attrs.lock() else { return std::ptr::null_mut() };
        if g.is_none() {
            *g = ComWrapper::new(AttrList::default()).to_com_ptr::<IAttributeList>();
        }
        g.as_ref().map(|p| p.as_ptr()).unwrap_or(std::ptr::null_mut())
    }
}

enum AttrValue {
    Int(i64),
    Float(f64),
    Text(Vec<u16>),
    Binary(Vec<u8>),
}

#[derive(Default)]
struct AttrList {
    values: Mutex<HashMap<String, AttrValue>>,
}

impl Class for AttrList {
    type Interfaces = (IAttributeList,);
}

fn attr_key(id: IAttributeList_::AttrID) -> String {
    if id.is_null() { String::new() } else { unsafe { CStr::from_ptr(id) }.to_string_lossy().into_owned() }
}

impl IAttributeListTrait for AttrList {
    unsafe fn setInt(&self, id: IAttributeList_::AttrID, value: int64) -> tresult {
        self.values
            .lock()
            .map(|mut m| m.insert(attr_key(id), AttrValue::Int(value)))
            .map_or(kResultFalse, |_| kResultOk)
    }
    unsafe fn getInt(&self, id: IAttributeList_::AttrID, value: *mut int64) -> tresult {
        match self.values.lock().ok().as_ref().and_then(|m| match m.get(&attr_key(id)) {
            Some(AttrValue::Int(v)) => Some(*v),
            _ => None,
        }) {
            Some(v) => {
                unsafe { *value = v };
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn setFloat(&self, id: IAttributeList_::AttrID, value: f64) -> tresult {
        self.values
            .lock()
            .map(|mut m| m.insert(attr_key(id), AttrValue::Float(value)))
            .map_or(kResultFalse, |_| kResultOk)
    }
    unsafe fn getFloat(&self, id: IAttributeList_::AttrID, value: *mut f64) -> tresult {
        match self.values.lock().ok().as_ref().and_then(|m| match m.get(&attr_key(id)) {
            Some(AttrValue::Float(v)) => Some(*v),
            _ => None,
        }) {
            Some(v) => {
                unsafe { *value = v };
                kResultOk
            }
            None => kResultFalse,
        }
    }
    unsafe fn setString(&self, id: IAttributeList_::AttrID, string: *const TChar) -> tresult {
        if string.is_null() {
            return kInvalidArgument;
        }
        let mut v = Vec::new();
        let mut p = string;
        unsafe {
            while *p != 0 && v.len() < 1 << 16 {
                v.push(*p);
                p = p.add(1);
            }
        }
        self.values.lock().map(|mut m| m.insert(attr_key(id), AttrValue::Text(v))).map_or(kResultFalse, |_| kResultOk)
    }
    unsafe fn getString(&self, id: IAttributeList_::AttrID, string: *mut TChar, sizeInBytes: uint32) -> tresult {
        let Ok(m) = self.values.lock() else { return kResultFalse };
        let Some(AttrValue::Text(v)) = m.get(&attr_key(id)) else { return kResultFalse };
        let cap = (sizeInBytes as usize / 2).saturating_sub(1);
        unsafe {
            for (i, c) in v.iter().take(cap).enumerate() {
                *string.add(i) = *c as TChar;
            }
            *string.add(v.len().min(cap)) = 0;
        }
        kResultOk
    }
    unsafe fn setBinary(&self, id: IAttributeList_::AttrID, data: *const c_void, sizeInBytes: uint32) -> tresult {
        let bytes = if data.is_null() {
            Vec::new()
        } else {
            unsafe { std::slice::from_raw_parts(data as *const u8, sizeInBytes as usize) }.to_vec()
        };
        self.values
            .lock()
            .map(|mut m| m.insert(attr_key(id), AttrValue::Binary(bytes)))
            .map_or(kResultFalse, |_| kResultOk)
    }
    unsafe fn getBinary(
        &self,
        id: IAttributeList_::AttrID,
        data: *mut *const c_void,
        sizeInBytes: *mut uint32,
    ) -> tresult {
        let Ok(m) = self.values.lock() else { return kResultFalse };
        let Some(AttrValue::Binary(v)) = m.get(&attr_key(id)) else { return kResultFalse };
        // The bytes stay owned by the list, valid while the message lives (as in the SDK).
        unsafe {
            *data = v.as_ptr() as *const c_void;
            *sizeInBytes = v.len() as uint32;
        }
        kResultOk
    }
}

/// Growable in-memory stream for plugin state.
#[derive(Default)]
struct MemStream {
    inner: Mutex<(Vec<u8>, usize)>,
}

impl Class for MemStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemStream {
    unsafe fn read(&self, buffer: *mut c_void, numBytes: int32, numBytesRead: *mut int32) -> tresult {
        let Ok(mut g) = self.inner.lock() else { return kResultFalse };
        let (data, pos) = &mut *g;
        let n = (numBytes.max(0) as usize).min(data.len().saturating_sub(*pos));
        unsafe { std::ptr::copy_nonoverlapping(data.as_ptr().add(*pos), buffer as *mut u8, n) };
        *pos += n;
        if !numBytesRead.is_null() {
            unsafe { *numBytesRead = n as int32 };
        }
        kResultOk
    }
    unsafe fn write(&self, buffer: *mut c_void, numBytes: int32, numBytesWritten: *mut int32) -> tresult {
        let Ok(mut g) = self.inner.lock() else { return kResultFalse };
        let (data, pos) = &mut *g;
        let n = numBytes.max(0) as usize;
        if data.len() < *pos + n {
            data.resize(*pos + n, 0);
        }
        unsafe { std::ptr::copy_nonoverlapping(buffer as *const u8, data.as_mut_ptr().add(*pos), n) };
        *pos += n;
        if !numBytesWritten.is_null() {
            unsafe { *numBytesWritten = n as int32 };
        }
        kResultOk
    }
    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut g) = self.inner.lock() else { return kResultFalse };
        let (data, cur) = &mut *g;
        let base = match mode {
            0 => 0i64,              // kIBSeekSet
            1 => *cur as i64,       // kIBSeekCur
            _ => data.len() as i64, // kIBSeekEnd
        };
        *cur = (base + pos).clamp(0, data.len() as i64) as usize;
        if !result.is_null() {
            unsafe { *result = *cur as int64 };
        }
        kResultOk
    }
    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(g) = self.inner.lock() else { return kResultFalse };
        if !pos.is_null() {
            unsafe { *pos = g.1 as int64 };
        }
        kResultOk
    }
}

fn stream_with(data: &[u8]) -> ComWrapper<MemStream> {
    ComWrapper::new(MemStream { inner: Mutex::new((data.to_vec(), 0)) })
}

fn stream_bytes(s: &ComWrapper<MemStream>) -> Vec<u8> {
    s.inner.lock().map(|g| g.0.clone()).unwrap_or_default()
}

/// Events for one process call (audio thread only; preallocated).
struct EventList {
    events: UnsafeCell<Vec<Event>>,
}

unsafe impl Send for EventList {}
unsafe impl Sync for EventList {}

impl Class for EventList {
    type Interfaces = (IEventList,);
}

impl IEventListTrait for EventList {
    unsafe fn getEventCount(&self) -> int32 {
        unsafe { (&*self.events.get()).len() as int32 }
    }
    unsafe fn getEvent(&self, index: int32, e: *mut Event) -> tresult {
        match unsafe { (&*self.events.get()).get(index as usize) } {
            Some(ev) => {
                unsafe { std::ptr::copy_nonoverlapping(ev, e, 1) };
                kResultOk
            }
            None => kInvalidArgument,
        }
    }
    unsafe fn addEvent(&self, _e: *mut Event) -> tresult {
        kResultOk // output events are ignored
    }
}

/// One parameter's changes within a block.
struct ParamQueue {
    id: UnsafeCell<ParamID>,
    value: UnsafeCell<ParamValue>,
}

unsafe impl Send for ParamQueue {}
unsafe impl Sync for ParamQueue {}

impl Class for ParamQueue {
    type Interfaces = (IParamValueQueue,);
}

impl IParamValueQueueTrait for ParamQueue {
    unsafe fn getParameterId(&self) -> ParamID {
        unsafe { *self.id.get() }
    }
    unsafe fn getPointCount(&self) -> int32 {
        1
    }
    unsafe fn getPoint(&self, index: int32, sampleOffset: *mut int32, value: *mut ParamValue) -> tresult {
        if index != 0 {
            return kInvalidArgument;
        }
        unsafe {
            *sampleOffset = 0;
            *value = *self.value.get();
        }
        kResultOk
    }
    unsafe fn addPoint(&self, _sampleOffset: int32, _value: ParamValue, _index: *mut int32) -> tresult {
        kResultFalse
    }
}

struct ParamChanges {
    queues: Vec<ComWrapper<ParamQueue>>,
    count: UnsafeCell<usize>,
}

unsafe impl Send for ParamChanges {}
unsafe impl Sync for ParamChanges {}

impl Class for ParamChanges {
    type Interfaces = (IParameterChanges,);
}

impl IParameterChangesTrait for ParamChanges {
    unsafe fn getParameterCount(&self) -> int32 {
        unsafe { *self.count.get() as int32 }
    }
    unsafe fn getParameterData(&self, index: int32) -> *mut IParamValueQueue {
        if (index as usize) < unsafe { *self.count.get() } {
            self.queues[index as usize]
                .as_com_ref::<IParamValueQueue>()
                .map(|r| r.as_ptr())
                .unwrap_or(std::ptr::null_mut())
        } else {
            std::ptr::null_mut()
        }
    }
    unsafe fn addParameterData(&self, _id: *const ParamID, _index: *mut int32) -> *mut IParamValueQueue {
        std::ptr::null_mut() // output parameter changes are ignored
    }
}

/// Edits made in the plugin's own window, waiting to reach the processor.
#[derive(Default)]
struct Handler {
    pending: Mutex<Vec<(ParamID, ParamValue)>>,
}

impl Class for Handler {
    type Interfaces = (IComponentHandler,);
}

impl IComponentHandlerTrait for Handler {
    unsafe fn beginEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn performEdit(&self, id: ParamID, valueNormalized: ParamValue) -> tresult {
        if let Ok(mut p) = self.pending.lock() {
            p.retain(|(i, _)| *i != id);
            p.push((id, valueNormalized));
        }
        kResultOk
    }
    unsafe fn endEdit(&self, _id: ParamID) -> tresult {
        kResultOk
    }
    unsafe fn restartComponent(&self, _flags: int32) -> tresult {
        kResultOk
    }
}

struct Frame {
    hwnd: isize,
}

impl Class for Frame {
    type Interfaces = (IPlugFrame,);
}

impl IPlugFrameTrait for Frame {
    unsafe fn resizeView(&self, view: *mut IPlugView, newSize: *mut ViewRect) -> tresult {
        let r = unsafe { &mut *newSize };
        editor::set_client_size(self.hwnd, r.right - r.left, r.bottom - r.top);
        if let Some(v) = unsafe { vst3::ComRef::from_raw(view) } {
            unsafe { v.onSize(newSize) };
        }
        kResultOk
    }
}

// ----- the plugin instance --------------------------------------------------

struct Bus {
    channels: usize,
}

struct AudioState {
    notes: NoteMap,
    in_bufs: Vec<Vec<Vec<f32>>>,
    out_bufs: Vec<Vec<Vec<f32>>>,
    in_ptrs: Vec<Vec<*mut f32>>,
    out_ptrs: Vec<Vec<*mut f32>>,
    in_buses: Vec<AudioBusBuffers>,
    out_buses: Vec<AudioBusBuffers>,
    context: ProcessContext,
}

pub struct Vst3Plugin {
    desc: PluginDesc,
    component: ComPtr<IComponent>,
    processor: ComPtr<IAudioProcessor>,
    controller: Option<ComPtr<IEditController>>,
    separate_controller: bool,
    connections: Option<(ComPtr<IConnectionPoint>, ComPtr<IConnectionPoint>)>,
    host: ComWrapper<HostApp>,
    handler: ComWrapper<Handler>,
    events: ComWrapper<EventList>,
    changes: ComWrapper<ParamChanges>,
    params: Vec<(ParamID, String)>,
    view: Mutex<Option<(ComPtr<IPlugView>, ComWrapper<Frame>)>>,
    audio: UnsafeCell<AudioState>,
    active: AtomicBool,
    _module: Module,
}

// COM objects here are used per the VST3 threading rules: the processor only from the
// audio thread (`process`), the controller and view only from the UI thread.
unsafe impl Send for Vst3Plugin {}
unsafe impl Sync for Vst3Plugin {}

fn bus_list(component: &ComPtr<IComponent>, dir: BusDirection) -> Vec<Bus> {
    let n = unsafe { component.getBusCount(MediaTypes_::kAudio as MediaType, dir) };
    (0..n)
        .map(|i| {
            let mut info: BusInfo = unsafe { std::mem::zeroed() };
            let ok = unsafe { component.getBusInfo(MediaTypes_::kAudio as MediaType, dir, i, &mut info) } == kResultOk;
            Bus { channels: if ok { info.channelCount.clamp(0, 32) as usize } else { 2 } }
        })
        .collect()
}

impl Vst3Plugin {
    pub fn load(desc: &PluginDesc, sample_rate: f32) -> Result<Self, HostError> {
        let module = open(&desc.path)?;
        let cid = tuid_from_hex(&desc.id).ok_or_else(|| HostError::NotAPlugin("bad VST3 class ID".into()))?;
        let host = ComWrapper::new(HostApp);
        let host_ptr = host.to_com_ptr::<FUnknown>().ok_or_else(|| HostError::Failed("host context".into()))?;
        unsafe {
            let mut obj: *mut c_void = std::ptr::null_mut();
            if module.factory.createInstance(cid.as_ptr(), IComponent::IID.as_ptr() as FIDString, &mut obj) != kResultOk
                || obj.is_null()
            {
                return Err(HostError::Failed("could not create the plugin".into()));
            }
            let component = ComPtr::from_raw(obj as *mut IComponent).unwrap();
            if component.initialize(host_ptr.as_ptr()) != kResultOk {
                return Err(HostError::Failed("the plugin refused to initialize".into()));
            }
            let processor = component
                .cast::<IAudioProcessor>()
                .ok_or_else(|| HostError::NotAPlugin("no audio processor".into()))?;

            // Controller: the same object, or a separate class.
            let (controller, separate) = match component.cast::<IEditController>() {
                Some(c) => (Some(c), false),
                None => {
                    let mut ccid: TUID = [0; 16];
                    let mut cobj: *mut c_void = std::ptr::null_mut();
                    if component.getControllerClassId(&mut ccid) == kResultOk
                        && module.factory.createInstance(
                            ccid.as_ptr(),
                            IEditController::IID.as_ptr() as FIDString,
                            &mut cobj,
                        ) == kResultOk
                        && !cobj.is_null()
                    {
                        let c = ComPtr::from_raw(cobj as *mut IEditController).unwrap();
                        c.initialize(host_ptr.as_ptr());
                        (Some(c), true)
                    } else {
                        (None, false)
                    }
                }
            };
            let connections = if separate {
                match (
                    component.cast::<IConnectionPoint>(),
                    controller.as_ref().and_then(|c| c.cast::<IConnectionPoint>()),
                ) {
                    (Some(a), Some(b)) => {
                        a.connect(b.as_ptr());
                        b.connect(a.as_ptr());
                        Some((a, b))
                    }
                    _ => None,
                }
            } else {
                None
            };
            let handler = ComWrapper::new(Handler::default());
            if let (Some(c), Some(h)) = (&controller, handler.to_com_ptr::<IComponentHandler>()) {
                c.setComponentHandler(h.as_ptr());
                // The controller starts from the processor's state.
                if separate {
                    let s = ComWrapper::new(MemStream::default());
                    if let Some(sp) = s.to_com_ptr::<IBStream>()
                        && component.getState(sp.as_ptr()) == kResultOk
                    {
                        sp.seek(0, 0, std::ptr::null_mut());
                        c.setComponentState(sp.as_ptr());
                    }
                }
            }

            // Stereo main buses where the plugin allows it, then activate the main buses.
            let ins = bus_list(&component, BusDirections_::kInput as BusDirection);
            let outs = bus_list(&component, BusDirections_::kOutput as BusDirection);
            let mut in_arr: Vec<SpeakerArrangement> = ins.iter().map(|_| SpeakerArr::kStereo).collect();
            let mut out_arr: Vec<SpeakerArrangement> = outs.iter().map(|_| SpeakerArr::kStereo).collect();
            processor.setBusArrangements(
                in_arr.as_mut_ptr(),
                in_arr.len() as i32,
                out_arr.as_mut_ptr(),
                out_arr.len() as i32,
            );
            let ins = bus_list(&component, BusDirections_::kInput as BusDirection);
            let outs = bus_list(&component, BusDirections_::kOutput as BusDirection);
            for (dir, n) in [(BusDirections_::kInput, ins.len()), (BusDirections_::kOutput, outs.len())] {
                for i in 0..n as i32 {
                    component.activateBus(MediaTypes_::kAudio as MediaType, dir as BusDirection, i, (i == 0) as TBool);
                }
            }
            if component.getBusCount(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection) > 0 {
                component.activateBus(MediaTypes_::kEvent as MediaType, BusDirections_::kInput as BusDirection, 0, 1);
            }
            let mut setup = ProcessSetup {
                processMode: ProcessModes_::kRealtime as int32,
                symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
                maxSamplesPerBlock: MAX_BLOCK as int32,
                sampleRate: sample_rate as f64,
            };
            processor.setupProcessing(&mut setup);
            component.setActive(1);
            processor.setProcessing(1);

            let params = match &controller {
                Some(c) => (0..c.getParameterCount().clamp(0, 100_000))
                    .filter_map(|i| {
                        let mut info: ParameterInfo = std::mem::zeroed();
                        (c.getParameterInfo(i, &mut info) == kResultOk).then(|| (info.id, wstr(&info.title)))
                    })
                    .collect(),
                None => Vec::new(),
            };

            let mk_bufs = |buses: &[Bus]| -> Vec<Vec<Vec<f32>>> {
                buses.iter().map(|b| (0..b.channels).map(|_| vec![0.0; MAX_BLOCK]).collect()).collect()
            };
            let in_bufs = mk_bufs(&ins);
            let out_bufs = mk_bufs(&outs);
            let audio = AudioState {
                notes: NoteMap::new(),
                in_ptrs: in_bufs.iter().map(|b| vec![std::ptr::null_mut(); b.len()]).collect(),
                out_ptrs: out_bufs.iter().map(|b| vec![std::ptr::null_mut(); b.len()]).collect(),
                in_buses: ins
                    .iter()
                    .map(|b| AudioBusBuffers {
                        numChannels: b.channels as i32,
                        silenceFlags: 0,
                        __field0: AudioBusBuffers__type0 { channelBuffers32: std::ptr::null_mut() },
                    })
                    .collect(),
                out_buses: outs
                    .iter()
                    .map(|b| AudioBusBuffers {
                        numChannels: b.channels as i32,
                        silenceFlags: 0,
                        __field0: AudioBusBuffers__type0 { channelBuffers32: std::ptr::null_mut() },
                    })
                    .collect(),
                in_bufs,
                out_bufs,
                context: std::mem::zeroed(),
            };
            let events = ComWrapper::new(EventList { events: UnsafeCell::new(Vec::with_capacity(MAX_EVENTS)) });
            let changes = ComWrapper::new(ParamChanges {
                queues: (0..MAX_PARAM_QUEUES)
                    .map(|_| ComWrapper::new(ParamQueue { id: UnsafeCell::new(0), value: UnsafeCell::new(0.0) }))
                    .collect(),
                count: UnsafeCell::new(0),
            });
            let mut desc = desc.clone();
            desc.inputs = ins.iter().map(|b| b.channels as u32).sum();
            desc.outputs = outs.iter().map(|b| b.channels as u32).sum();
            Ok(Self {
                desc,
                component,
                processor,
                controller,
                separate_controller: separate,
                connections,
                host,
                handler,
                events,
                changes,
                params,
                view: Mutex::new(None),
                audio: UnsafeCell::new(audio),
                active: AtomicBool::new(true),
                _module: module,
            })
        }
    }
}

impl Drop for Vst3Plugin {
    fn drop(&mut self) {
        crate::pin::pin_new_modules();
        self.close_editor();
        unsafe {
            if self.active.swap(false, Ordering::AcqRel) {
                self.processor.setProcessing(0);
                self.component.setActive(0);
            }
            if let Some((a, b)) = self.connections.take() {
                a.disconnect(b.as_ptr());
                b.disconnect(a.as_ptr());
            }
            if let Some(c) = &self.controller {
                c.setComponentHandler(std::ptr::null_mut());
                if self.separate_controller {
                    c.terminate();
                }
            }
            self.component.terminate();
        }
        let _ = &self.host;
    }
}

const STATE_MAGIC: &[u8; 4] = b"SDV3";

impl PluginInstance for Vst3Plugin {
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
        match (&self.controller, self.params.get(index)) {
            (Some(c), Some((id, _))) => unsafe { c.getParamNormalized(*id) as f32 },
            _ => 0.0,
        }
    }

    fn param_display(&self, index: usize) -> String {
        match (&self.controller, self.params.get(index)) {
            (Some(c), Some((id, _))) => unsafe {
                let mut s: String128 = [0; 128];
                if c.getParamStringByValue(*id, c.getParamNormalized(*id), &mut s) == kResultOk {
                    wstr(&s)
                } else {
                    String::new()
                }
            },
            _ => String::new(),
        }
    }

    fn set_param_value(&self, index: usize, value: f32) {
        if let (Some(c), Some((id, _))) = (&self.controller, self.params.get(index)) {
            let v = value.clamp(0.0, 1.0) as f64;
            unsafe { c.setParamNormalized(*id, v) };
            unsafe { self.handler.performEdit(*id, v) };
        }
    }

    fn save_state(&self) -> Option<Vec<u8>> {
        let comp = ComWrapper::new(MemStream::default());
        let ctrl = ComWrapper::new(MemStream::default());
        unsafe {
            self.component.getState(comp.to_com_ptr::<IBStream>()?.as_ptr());
            if let Some(c) = &self.controller {
                c.getState(ctrl.to_com_ptr::<IBStream>()?.as_ptr());
            }
        }
        let (a, b) = (stream_bytes(&comp), stream_bytes(&ctrl));
        let mut out = STATE_MAGIC.to_vec();
        out.extend_from_slice(&(a.len() as u32).to_le_bytes());
        out.extend_from_slice(&a);
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(&b);
        Some(out)
    }

    fn load_state(&self, data: &[u8]) -> bool {
        let Some(rest) = data.strip_prefix(STATE_MAGIC) else { return false };
        let take = |r: &[u8]| -> Option<(Vec<u8>, usize)> {
            let n = u32::from_le_bytes(r.get(..4)?.try_into().ok()?) as usize;
            Some((r.get(4..4 + n)?.to_vec(), 4 + n))
        };
        let Some((comp, used)) = take(rest) else { return false };
        let ctrl = take(&rest[used..]).map(|x| x.0).unwrap_or_default();
        unsafe {
            let s = stream_with(&comp);
            let Some(sp) = s.to_com_ptr::<IBStream>() else { return false };
            self.component.setState(sp.as_ptr());
            if let Some(c) = &self.controller {
                let s2 = stream_with(&comp);
                if let Some(p) = s2.to_com_ptr::<IBStream>() {
                    c.setComponentState(p.as_ptr());
                }
                if !ctrl.is_empty() {
                    let s3 = stream_with(&ctrl);
                    if let Some(p) = s3.to_com_ptr::<IBStream>() {
                        c.setState(p.as_ptr());
                    }
                }
            }
        }
        true
    }

    fn has_editor(&self) -> bool {
        self.controller.is_some()
    }

    fn open_editor(&self, parent: isize) -> Option<(i32, i32)> {
        let c = self.controller.as_ref()?;
        unsafe {
            let view = ComPtr::from_raw(c.createView(c"editor".as_ptr()))?;
            if view.isPlatformTypeSupported(kPlatformTypeHWND) != kResultOk {
                return None;
            }
            let frame = ComWrapper::new(Frame { hwnd: parent });
            if let Some(f) = frame.to_com_ptr::<IPlugFrame>() {
                view.setFrame(f.as_ptr());
            }
            if view.attached(parent as *mut c_void, kPlatformTypeHWND) != kResultOk {
                return None;
            }
            let mut r: ViewRect = std::mem::zeroed();
            view.getSize(&mut r);
            let size = ((r.right - r.left).max(100), (r.bottom - r.top).max(60));
            if let Ok(mut g) = self.view.lock() {
                *g = Some((view, frame));
            }
            Some(size)
        }
    }

    fn close_editor(&self) {
        if let Some((view, _frame)) = self.view.lock().ok().and_then(|mut g| g.take()) {
            unsafe {
                view.removed();
                view.setFrame(std::ptr::null_mut());
            }
        }
    }

    fn process(&self, events: &[TimedEvent], left: &mut [f32], right: &mut [f32]) {
        let st = unsafe { &mut *self.audio.get() };
        let n = left.len().min(right.len()).min(MAX_BLOCK);

        // Notes.
        let list = unsafe { &mut *self.events.events.get() };
        list.clear();
        for e in events {
            let Some(m) = st.notes.midi(&e.event) else { continue };
            let mut push = |kind: u16, pitch: i16, velocity: f32| {
                if list.len() < MAX_EVENTS {
                    let mut ev: Event = unsafe { std::mem::zeroed() };
                    ev.sampleOffset = (e.offset as i32).min(n as i32 - 1).max(0);
                    ev.r#type = kind;
                    if kind == Event_::EventTypes_::kNoteOnEvent as u16 {
                        ev.__field0.noteOn =
                            NoteOnEvent { channel: 0, pitch, tuning: 0.0, velocity, length: 0, noteId: -1 };
                    } else {
                        ev.__field0.noteOff = NoteOffEvent { channel: 0, pitch, velocity, noteId: -1, tuning: 0.0 };
                    }
                    list.push(ev);
                }
            };
            match m[0] {
                0x90 => push(Event_::EventTypes_::kNoteOnEvent as u16, m[1] as i16, m[2] as f32 / 127.0),
                0x80 => push(Event_::EventTypes_::kNoteOffEvent as u16, m[1] as i16, 0.5),
                // All notes / sound off: VST3 has no such event, so release every key.
                _ => (0..128).for_each(|k| push(Event_::EventTypes_::kNoteOffEvent as u16, k, 0.0)),
            }
        }

        // Parameter edits from the UI or the plugin's window.
        let count = unsafe { &mut *self.changes.count.get() };
        *count = 0;
        if let Ok(mut pending) = self.handler.pending.try_lock() {
            for (id, value) in pending.drain(..).take(MAX_PARAM_QUEUES) {
                let q = &self.changes.queues[*count];
                unsafe {
                    *q.id.get() = id;
                    *q.value.get() = value;
                }
                *count += 1;
            }
        }

        // Buffers: main input gets our stereo signal, everything else silence.
        for (b, bus) in st.in_bufs.iter_mut().enumerate() {
            for (c, buf) in bus.iter_mut().enumerate() {
                match (b, c) {
                    (0, 0) => buf[..n].copy_from_slice(&left[..n]),
                    (0, 1) => buf[..n].copy_from_slice(&right[..n]),
                    _ => buf[..n].fill(0.0),
                }
                st.in_ptrs[b][c] = buf.as_mut_ptr();
            }
            st.in_buses[b].__field0.channelBuffers32 = st.in_ptrs[b].as_mut_ptr();
            st.in_buses[b].silenceFlags = 0;
        }
        for (b, bus) in st.out_bufs.iter_mut().enumerate() {
            for (c, buf) in bus.iter_mut().enumerate() {
                buf[..n].fill(0.0);
                st.out_ptrs[b][c] = buf.as_mut_ptr();
            }
            st.out_buses[b].__field0.channelBuffers32 = st.out_ptrs[b].as_mut_ptr();
            st.out_buses[b].silenceFlags = 0;
        }

        let t = transport::read();
        let beats_per_bar = t.ts_num.max(1) as f64 * 4.0 / t.ts_den.max(1) as f64;
        st.context.state = ((if t.playing { ProcessContext_::StatesAndFlags_::kPlaying } else { 0 })
            | ProcessContext_::StatesAndFlags_::kTempoValid
            | ProcessContext_::StatesAndFlags_::kTimeSigValid
            | ProcessContext_::StatesAndFlags_::kProjectTimeMusicValid
            | ProcessContext_::StatesAndFlags_::kBarPositionValid) as u32;
        st.context.sampleRate = t.sample_rate;
        st.context.projectTimeSamples = t.sample_pos;
        st.context.projectTimeMusic = t.ppq_pos;
        st.context.barPositionMusic = (t.ppq_pos / beats_per_bar).floor() * beats_per_bar;
        st.context.tempo = t.bpm;
        st.context.timeSigNumerator = t.ts_num as i32;
        st.context.timeSigDenominator = t.ts_den as i32;

        let mut data = ProcessData {
            processMode: ProcessModes_::kRealtime as int32,
            symbolicSampleSize: SymbolicSampleSizes_::kSample32 as int32,
            numSamples: n as int32,
            numInputs: st.in_buses.len() as int32,
            numOutputs: st.out_buses.len() as int32,
            inputs: st.in_buses.as_mut_ptr(),
            outputs: st.out_buses.as_mut_ptr(),
            inputParameterChanges: self
                .changes
                .as_com_ref::<IParameterChanges>()
                .map(|r| r.as_ptr())
                .unwrap_or(std::ptr::null_mut()),
            outputParameterChanges: std::ptr::null_mut(),
            inputEvents: self.events.as_com_ref::<IEventList>().map(|r| r.as_ptr()).unwrap_or(std::ptr::null_mut()),
            outputEvents: std::ptr::null_mut(),
            processContext: &mut st.context,
        };
        unsafe { self.processor.process(&mut data) };

        match st.out_bufs.first() {
            Some(bus) if bus.len() >= 2 => {
                left[..n].copy_from_slice(&bus[0][..n]);
                right[..n].copy_from_slice(&bus[1][..n]);
            }
            Some(bus) if bus.len() == 1 => {
                left[..n].copy_from_slice(&bus[0][..n]);
                right[..n].copy_from_slice(&bus[0][..n]);
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

    #[test]
    fn class_ids_round_trip_and_streams_work() {
        let t: TUID = std::array::from_fn(|i| (i * 17) as c_char);
        assert_eq!(tuid_from_hex(&tuid_hex(&t)), Some(t));
        assert!(tuid_from_hex("xyz").is_none());
        let s = stream_with(b"hello");
        let p = s.to_com_ptr::<IBStream>().unwrap();
        let mut buf = [0u8; 3];
        let mut got = 0;
        unsafe {
            p.read(buf.as_mut_ptr() as *mut c_void, 3, &mut got);
            assert_eq!((&buf, got), (b"hel", 3));
            let mut w = 0;
            p.seek(0, 2, std::ptr::null_mut());
            p.write(b"!!".as_ptr() as *mut c_void, 2, &mut w);
        }
        assert_eq!(stream_bytes(&s), b"hello!!");
        assert!(matches!(describe(Path::new("C:\\missing\\x.vst3")), Err(HostError::Load(_))));
    }
}
