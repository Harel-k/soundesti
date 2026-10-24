//! Loads a plugin the way the app does (audio on its own thread, window messages pumped on
//! the main thread), plays one note (instruments) or a tone (effects), then closes it.
//! Usage: `cargo run -p soundesti-plugin-host --example probe -- <vst2|vst3|clap> <path> [seconds]`
//! Set SOUNDESTI_VST_LOG=1 to print every plugin-to-host call.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

use soundesti_plugin_api::{NoteEvent, TimedEvent};
use soundesti_plugin_host::{PluginFormat, describe, load};

#[link(name = "ole32")]
unsafe extern "system" {
    fn OleInitialize(reserved: *mut std::ffi::c_void) -> i32;
}

/// Handles pending window messages for `ms`, as the app's event loop does.
fn pump(ms: u64) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage,
    };
    let end = Instant::now() + Duration::from_millis(ms);
    loop {
        let mut msg: MSG = unsafe { std::mem::zeroed() };
        while unsafe { PeekMessageW(&mut msg, std::ptr::null_mut(), 0, 0, PM_REMOVE) } != 0 {
            unsafe {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        if Instant::now() >= end {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn main() {
    unsafe { OleInitialize(std::ptr::null_mut()) };
    let args: Vec<String> = std::env::args().collect();
    let format = match args.get(1).map(String::as_str) {
        Some("vst3") => PluginFormat::Vst3,
        Some("clap") => PluginFormat::Clap,
        _ => PluginFormat::Vst2,
    };
    let path = std::path::PathBuf::from(&args[2]);
    let secs: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(2);
    let descs = match describe(format, &path) {
        Ok(d) => d,
        Err(e) => {
            println!("describe failed: {e}");
            std::process::exit(1);
        }
    };
    for d in descs {
        println!(
            "found: {} by {} ({}, instrument: {}, {} in / {} out, id {})",
            d.name,
            d.vendor,
            d.format.label(),
            d.is_instrument,
            d.inputs,
            d.outputs,
            d.id
        );
        let p = match load(&d, 48_000.0) {
            Ok(p) => p,
            Err(e) => {
                println!("  load failed: {e}");
                continue;
            }
        };
        println!("  params: {}, editor: {}", p.param_count(), p.has_editor());
        for i in 0..p.param_count().min(4) {
            println!("    {}: {} = {}", i, p.param_name(i), p.param_display(i));
        }

        // Audio thread: 10 ms blocks, note on at the start, off after half the time.
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU32::new(0));
        let audio = {
            let (p, stop, peak, instrument) = (p.clone(), stop.clone(), peak.clone(), d.is_instrument);
            std::thread::spawn(move || {
                let mut block = 0usize;
                while !stop.load(Ordering::Relaxed) {
                    let (mut l, mut r) = (vec![0.0f32; 480], vec![0.0f32; 480]);
                    if !instrument {
                        for (i, (a, b)) in l.iter_mut().zip(r.iter_mut()).enumerate() {
                            *a = ((block * 480 + i) as f32 * 0.0575).sin() * 0.5;
                            *b = *a;
                        }
                    }
                    let ev = match block {
                        0 => vec![TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key: 60, velocity: 0.8 } }],
                        b if b == secs as usize * 50 => vec![TimedEvent { offset: 0, event: NoteEvent::Off { id: 1 } }],
                        _ => vec![],
                    };
                    p.process(&ev, &mut l, &mut r);
                    let m = l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
                    peak.fetch_max(m.to_bits(), Ordering::Relaxed);
                    block += 1;
                    std::thread::sleep(Duration::from_millis(10));
                }
            })
        };
        pump(secs * 1000);
        let state = p.save_state().map(|s| s.len()).unwrap_or(0);
        println!(
            "  ran {secs} s: output peak {:.4}, saved state {state} bytes",
            f32::from_bits(peak.load(Ordering::Relaxed))
        );
        // Same order as the app: stop audio, then close the plugin on the UI thread.
        stop.store(true, Ordering::Relaxed);
        audio.join().unwrap();
        let t = Instant::now();
        drop(p);
        println!("  closed in {:.1} s", t.elapsed().as_secs_f32());
        for s in 1..=5 {
            pump(2000);
            println!("  still alive {}s after close", s * 2);
        }
    }
    println!("exiting");
    std::process::exit(0);
}
