//! Finding plugins. Each file is inspected in a child process (`soundesti.exe
//! --scan-plugin <format> <path>`) so a crashing or hanging plugin can't take Soundesti
//! down; results are cached by path and modification time.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{PluginDesc, PluginFormat};

/// Command-line flag of the scanner child process.
pub const SCAN_FLAG: &str = "--scan-plugin";
const TIMEOUT: Duration = Duration::from_secs(30);
/// Bump when the scanner learns new formats, so old `not supported` results are redone.
pub const SCANNER_VERSION: u32 = 1;

/// The standard plugin folders on Windows, plus FL Studio's VSTi folder when FL is installed.
pub fn default_dirs() -> Vec<(PluginFormat, PathBuf)> {
    let pf = std::env::var_os("ProgramFiles").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("C:\\Program Files"));
    let common = std::env::var_os("CommonProgramFiles").map(PathBuf::from).unwrap_or_else(|| pf.join("Common Files"));
    let mut dirs = vec![
        (PluginFormat::Vst3, common.join("VST3")),
        (PluginFormat::Clap, common.join("CLAP")),
        (PluginFormat::Vst2, pf.join("VSTPlugins")),
        (PluginFormat::Vst2, pf.join("Steinberg").join("VSTPlugins")),
        (PluginFormat::Vst2, common.join("VST2")),
        (PluginFormat::Vst2, common.join("Steinberg").join("VST2")),
    ];
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push((PluginFormat::Clap, PathBuf::from(local).join("Programs").join("Common").join("CLAP")));
    }
    if let Ok(rd) = std::fs::read_dir(pf.join("Image-Line")) {
        for e in rd.flatten() {
            let vsti = e.path().join("System").join("Plugin").join("VSTi").join("x64");
            if e.file_name().to_string_lossy().starts_with("FL Studio") && vsti.is_dir() {
                dirs.push((PluginFormat::Vst2, vsti));
            }
        }
    }
    dirs
}

/// Plugin files under `dir` for `format`.
pub fn find_files(format: PluginFormat, dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(format, dir, 0, &mut out);
    out.sort();
    out
}

fn walk(format: PluginFormat, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let ext = p.extension().map(|x| x.to_string_lossy().to_lowercase()).unwrap_or_default();
        match format {
            // A .vst3 is either a single file or a bundle folder; both are loaded by path.
            PluginFormat::Vst3 if ext == "vst3" => out.push(p),
            PluginFormat::Clap if ext == "clap" && p.is_file() => out.push(p),
            PluginFormat::Vst2 if ext == "dll" && p.is_file() => out.push(p),
            _ if p.is_dir() && depth < 6 => walk(format, &p, depth + 1, out),
            _ => {}
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheEntry {
    pub format: PluginFormat,
    pub path: PathBuf,
    pub modified: u64,
    pub plugins: Vec<PluginDesc>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PluginCache {
    pub entries: Vec<CacheEntry>,
    /// Extra folders the user added.
    #[serde(default)]
    pub user_dirs: Vec<(PluginFormat, PathBuf)>,
    /// Scanner that produced `entries`.
    #[serde(default)]
    pub version: u32,
}

impl PluginCache {
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Soundesti").join("plugins.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        if let Some(p) = Self::path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(s) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, s);
            }
        }
    }

    /// Every plugin found, instruments first, then by name.
    pub fn plugins(&self) -> Vec<PluginDesc> {
        let mut v: Vec<PluginDesc> = self.entries.iter().flat_map(|e| e.plugins.iter().cloned()).collect();
        v.sort_by(|a, b| {
            b.is_instrument.cmp(&a.is_instrument).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        // The same plugin is often installed in two folders (FL copies its VSTi into VST2).
        v.dedup_by(|a, b| a.format == b.format && a.id == b.id && a.name == b.name);
        v
    }

    pub fn failures(&self) -> impl Iterator<Item = &CacheEntry> {
        self.entries.iter().filter(|e| e.error.is_some())
    }

    pub fn all_dirs(&self) -> Vec<(PluginFormat, PathBuf)> {
        let mut d = default_dirs();
        d.extend(self.user_dirs.iter().cloned());
        d
    }
}

fn modified(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Rescans every folder. Unchanged files keep their cached result. `progress` gets
/// (done, total, current file) and returning false stops the scan.
pub fn rescan(cache: &mut PluginCache, scanner_exe: &Path, mut progress: impl FnMut(usize, usize, &Path) -> bool) {
    let files: Vec<(PluginFormat, PathBuf)> = cache
        .all_dirs()
        .into_iter()
        .flat_map(|(fmt, dir)| find_files(fmt, &dir).into_iter().map(move |p| (fmt, p)))
        .collect();
    let total = files.len();
    if cache.version != SCANNER_VERSION {
        cache.entries.clear();
        cache.version = SCANNER_VERSION;
    }
    let mut entries = Vec::with_capacity(total);
    for (i, (format, path)) in files.into_iter().enumerate() {
        if !progress(i, total, &path) {
            return;
        }
        let m = modified(&path);
        if let Some(old) = cache.entries.iter().find(|e| e.path == path && e.format == format && e.modified == m) {
            entries.push(old.clone());
            continue;
        }
        let (plugins, error) = match scan_in_child(scanner_exe, format, &path) {
            Ok(p) => (p, None),
            Err(e) => (Vec::new(), Some(e)),
        };
        entries.push(CacheEntry { format, path, modified: m, plugins, error });
    }
    cache.entries = entries;
    progress(total, total, Path::new(""));
}

pub fn scan_in_child(exe: &Path, format: PluginFormat, path: &Path) -> Result<Vec<PluginDesc>, String> {
    let fmt = match format {
        PluginFormat::Vst2 => "vst2",
        PluginFormat::Vst3 => "vst3",
        PluginFormat::Clap => "clap",
    };
    let mut child = Command::new(exe)
        .arg(SCAN_FLAG)
        .arg(fmt)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start the scanner: {e}"))?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut out = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut out);
                }
                let line = out.lines().rev().find(|l| l.starts_with('{')).unwrap_or("");
                let reply: ScanReply = serde_json::from_str(line).map_err(|_| {
                    if status.success() {
                        "the scanner gave no answer".to_string()
                    } else {
                        format!("the plugin crashed while loading ({status})")
                    }
                })?;
                return match reply.error {
                    Some(e) => Err(e),
                    None => Ok(reply.plugins),
                };
            }
            Ok(None) if started.elapsed() > TIMEOUT => {
                let _ = child.kill();
                return Err("the plugin took too long to load".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => return Err(e.to_string()),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ScanReply {
    plugins: Vec<PluginDesc>,
    error: Option<String>,
}

/// Body of the scanner child process: `args` are what follows `--scan-plugin`.
pub fn child_main(args: &[String]) -> i32 {
    let format = match args.first().map(String::as_str) {
        Some("vst2") => PluginFormat::Vst2,
        Some("vst3") => PluginFormat::Vst3,
        Some("clap") => PluginFormat::Clap,
        _ => return 2,
    };
    let Some(path) = args.get(1) else { return 2 };
    let reply = match crate::describe(format, Path::new(path)) {
        Ok(plugins) => ScanReply { plugins, error: None },
        Err(e) => ScanReply { plugins: Vec::new(), error: Some(e.to_string()) },
    };
    println!("{}", serde_json::to_string(&reply).unwrap_or_default());
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_plugin_files_by_format() {
        let root = std::env::temp_dir().join(format!("soundesti-scan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Vendor").join("Synth.vst3").join("Contents")).unwrap();
        std::fs::write(root.join("Single.vst3"), b"").unwrap();
        std::fs::write(root.join("Vendor").join("Old.dll"), b"").unwrap();
        std::fs::write(root.join("Thing.clap"), b"").unwrap();
        let names = |v: Vec<PathBuf>| {
            v.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>()
        };
        assert_eq!(names(find_files(PluginFormat::Vst3, &root)), ["Single.vst3", "Synth.vst3"]);
        assert_eq!(names(find_files(PluginFormat::Vst2, &root)), ["Old.dll"]);
        assert_eq!(names(find_files(PluginFormat::Clap, &root)), ["Thing.clap"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn cache_lists_instruments_first() {
        let d = |name: &str, inst| PluginDesc {
            format: PluginFormat::Vst2,
            path: name.into(),
            id: name.into(),
            name: name.into(),
            vendor: String::new(),
            is_instrument: inst,
            inputs: 0,
            outputs: 2,
        };
        let cache = PluginCache {
            entries: vec![CacheEntry {
                format: PluginFormat::Vst2,
                path: "x".into(),
                modified: 0,
                plugins: vec![d("b fx", false), d("Zeta", true), d("alpha", true), d("alpha", true)],
                error: None,
            }],
            user_dirs: vec![],
            version: SCANNER_VERSION,
        };
        let names: Vec<String> = cache.plugins().into_iter().map(|p| p.name).collect();
        assert_eq!(names, ["alpha", "Zeta", "b fx"]);
    }
}
