//! Keeps plugin DLLs loaded until the process exits.
//!
//! Some plugins unload helper DLLs of their own when an instance closes while timers or
//! windows created by that code are still alive; the next tick then jumps into unmapped
//! memory and crashes the host. FL Studio VSTi does this with its FLEngine_x64*.dll
//! about ten seconds after its last instance closes. Pinning every module that appeared
//! after the first plugin load (GET_MODULE_HANDLE_EX_FLAG_PIN) means Windows never
//! unloads them, so that late code still finds itself in memory.

use std::collections::HashSet;
use std::sync::Mutex;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
};
use windows_sys::Win32::System::ProcessStatus::EnumProcessModules;
use windows_sys::Win32::System::Threading::GetCurrentProcess;

struct State {
    baseline: Option<HashSet<usize>>,
    pinned: Option<HashSet<usize>>,
}

static STATE: Mutex<State> = Mutex::new(State { baseline: None, pinned: None });

fn modules() -> Vec<usize> {
    let mut buf: Vec<HMODULE> = vec![std::ptr::null_mut(); 1024];
    let mut needed = 0u32;
    loop {
        let bytes = (buf.len() * std::mem::size_of::<HMODULE>()) as u32;
        let ok = unsafe { EnumProcessModules(GetCurrentProcess(), buf.as_mut_ptr(), bytes, &mut needed) };
        if ok == 0 {
            return Vec::new();
        }
        let count = needed as usize / std::mem::size_of::<HMODULE>();
        if count <= buf.len() {
            return buf[..count].iter().map(|h| *h as usize).collect();
        }
        buf.resize(count + 64, std::ptr::null_mut());
    }
}

/// Records the modules loaded before any plugin (the app's own and the system's).
pub fn remember_baseline() {
    if let Ok(mut s) = STATE.lock()
        && s.baseline.is_none()
    {
        s.baseline = Some(modules().into_iter().collect());
    }
}

/// Pins every module that is not part of the baseline and not pinned yet.
pub fn pin_new_modules() {
    let Ok(mut s) = STATE.lock() else { return };
    let Some(baseline) = s.baseline.clone() else { return };
    let pinned = s.pinned.get_or_insert_with(HashSet::new);
    for m in modules() {
        if baseline.contains(&m) || pinned.contains(&m) {
            continue;
        }
        let mut out: HMODULE = std::ptr::null_mut();
        // FROM_ADDRESS: the module base address identifies the module.
        let ok = unsafe {
            GetModuleHandleExW(
                GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                m as *const u16,
                &mut out,
            )
        };
        if ok != 0 {
            pinned.insert(m);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_and_pins_modules() {
        assert!(modules().len() > 3, "the process has modules");
        remember_baseline();
        // Load something new, then pin it; pinning twice is harmless.
        let lib = unsafe { libloading::Library::new("dbghelp.dll") };
        pin_new_modules();
        pin_new_modules();
        drop(lib);
    }
}
