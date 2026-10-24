//! Native windows that host plugin editors. Each editor gets a small top-level window
//! owned by Soundesti's main window (so it stays on top of it and minimizes with it).
//! Must be used from the UI thread; the main event loop pumps its messages.

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BLACK_BRUSH, GetStockObject, HBRUSH};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const CLASS_NAME: &str = "SoundestiPluginEditor";
const STYLE: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;

static CLASS_REGISTERED: AtomicBool = AtomicBool::new(false);
/// Windows whose close button was pressed (the host closes the plugin editor first).
static CLOSE_REQUESTS: Mutex<Vec<isize>> = Mutex::new(Vec::new());
/// Plugin instance key -> editor window, for plugin-initiated resizes.
static RESIZE_TARGETS: Mutex<Option<HashMap<usize, isize>>> = Mutex::new(None);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLOSE {
        // Don't destroy yet: the plugin's view must be detached first.
        unsafe { ShowWindow(hwnd, SW_HIDE) };
        if let Ok(mut v) = CLOSE_REQUESTS.lock() {
            v.push(hwnd as isize);
        }
        return 0;
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn register_class() {
    if CLASS_REGISTERED.swap(true, Ordering::AcqRel) {
        return;
    }
    let name = wide(CLASS_NAME);
    let wc = WNDCLASSW {
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: unsafe { GetModuleHandleW(std::ptr::null()) },
        hIcon: std::ptr::null_mut(),
        hCursor: unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) },
        hbrBackground: unsafe { GetStockObject(BLACK_BRUSH) } as HBRUSH,
        lpszMenuName: std::ptr::null(),
        lpszClassName: name.as_ptr(),
    };
    unsafe { RegisterClassW(&wc) };
}

/// One plugin editor window.
pub struct EditorWindow {
    hwnd: HWND,
}

impl EditorWindow {
    /// Creates a hidden window owned by `owner`; show it after the plugin attached its view.
    pub fn create(owner: isize, title: &str) -> Option<Self> {
        register_class();
        let class = wide(CLASS_NAME);
        let title = wide(title);
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOOLWINDOW,
                class.as_ptr(),
                title.as_ptr(),
                STYLE,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                400,
                300,
                owner as HWND,
                std::ptr::null_mut(),
                GetModuleHandleW(std::ptr::null()),
                std::ptr::null(),
            )
        };
        (!hwnd.is_null()).then_some(Self { hwnd })
    }

    pub fn handle(&self) -> isize {
        self.hwnd as isize
    }

    /// Sizes the window so its client area is `w` x `h` and shows it.
    pub fn show_with_client_size(&self, w: i32, h: i32) {
        set_client_size(self.hwnd as isize, w, h);
        unsafe {
            ShowWindow(self.hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(self.hwnd);
        }
    }

    pub fn focus(&self) {
        unsafe {
            ShowWindow(self.hwnd, SW_SHOWNORMAL);
            SetForegroundWindow(self.hwnd);
        }
    }
}

impl Drop for EditorWindow {
    fn drop(&mut self) {
        unsafe { DestroyWindow(self.hwnd) };
    }
}

fn set_client_size(hwnd: isize, w: i32, h: i32) {
    let mut r = RECT { left: 0, top: 0, right: w.max(50), bottom: h.max(30) };
    unsafe {
        AdjustWindowRectEx(&mut r, STYLE, 0, WS_EX_TOOLWINDOW);
        SetWindowPos(
            hwnd as HWND,
            std::ptr::null_mut(),
            0,
            0,
            r.right - r.left,
            r.bottom - r.top,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
}

/// Editor windows whose close button was pressed since the last call.
pub fn take_close_requests() -> Vec<isize> {
    CLOSE_REQUESTS.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}

pub fn register_resize_target(key: usize, hwnd: isize) {
    if let Ok(mut m) = RESIZE_TARGETS.lock() {
        m.get_or_insert_with(HashMap::new).insert(key, hwnd);
    }
}

pub fn unregister_resize_target(key: usize) {
    if let Ok(mut m) = RESIZE_TARGETS.lock()
        && let Some(m) = m.as_mut()
    {
        m.remove(&key);
    }
}

/// A plugin asked for a new editor size.
pub fn resize_for(key: usize, w: i32, h: i32) {
    let hwnd = RESIZE_TARGETS.lock().ok().and_then(|m| m.as_ref().and_then(|m| m.get(&key).copied()));
    if let Some(hwnd) = hwnd {
        set_client_size(hwnd, w, h);
    }
}
