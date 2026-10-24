//! Install / verify / repair / uninstall logic, independent of the UI.
//!
//! Per-user install (no admin): files in `%LOCALAPPDATA%\Programs\Soundesti`, an
//! "Apps & features" entry under HKCU, optional shortcuts and `.soundesti` association.
//! Everything written is recorded in `install-manifest.json` so repair and uninstall
//! touch exactly the installer's own files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use soundesti_updater::sha256_hex;
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

use crate::payload::Entry;

pub const APP_NAME: &str = "Soundesti";
pub const APP_EXE: &str = "soundesti.exe";
pub const UNINSTALLER_EXE: &str = "Uninstall Soundesti.exe";
pub const MANIFEST: &str = "install-manifest.json";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Where system integration is written. Tests point these at throwaway keys.
#[derive(Clone, Debug)]
pub struct Target {
    pub uninstall_key: String,
    pub classes_key: String,
    /// Create shortcuts / associations / registry entries (off in unit tests).
    pub system: bool,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            uninstall_key: format!(r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{APP_NAME}"),
            classes_key: r"Software\Classes".into(),
            system: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Options {
    pub dir: PathBuf,
    pub desktop_shortcut: bool,
    pub start_menu_shortcut: bool,
    pub associate_files: bool,
}

impl Options {
    pub fn default_for_user() -> Self {
        let base =
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Soundesti"));
        Self {
            dir: base.join("Programs").join(APP_NAME),
            desktop_shortcut: true,
            start_menu_shortcut: true,
            associate_files: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    pub options: Options,
    pub files: Vec<ManifestFile>,
    #[serde(default)]
    pub shortcuts: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub dir: PathBuf,
    pub version: String,
    pub manifest: Option<Manifest>,
}

/// Looks up an existing installation in the registry (falls back to the default folder).
pub fn detect(t: &Target) -> Option<Installed> {
    let dir = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(&t.uninstall_key)
        .ok()
        .and_then(|k| k.get_value::<String, _>("InstallLocation").ok())
        .map(PathBuf::from)
        .or_else(|| {
            let d = Options::default_for_user().dir;
            d.join(MANIFEST).exists().then_some(d)
        })?;
    let manifest = read_manifest(&dir);
    if manifest.is_none() && !dir.join(APP_EXE).exists() {
        return None;
    }
    let version = manifest.as_ref().map(|m| m.version.clone()).unwrap_or_else(|| "unknown".into());
    Some(Installed { dir, version, manifest })
}

pub fn read_manifest(dir: &Path) -> Option<Manifest> {
    std::fs::read(dir.join(MANIFEST)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_atomic(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    let tmp = path.with_extension("sdtmp");
    std::fs::write(&tmp, data).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    // A file can stay locked for a few seconds while Soundesti is closing (the in-app
    // updater quits the app right after launching us), so retry before giving up.
    let mut attempts = 0;
    loop {
        match std::fs::rename(&tmp, path) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied && attempts < 60 => {
                attempts += 1;
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(if e.kind() == std::io::ErrorKind::PermissionDenied {
                    format!("{} is in use. Please close Soundesti and try again.", path.display())
                } else {
                    format!("cannot replace {}: {e}", path.display())
                });
            }
        }
    }
}

/// Installs (or reinstalls/repairs) `payload` with `opts`. `self_exe` is copied in as the
/// uninstaller. `progress(fraction, message)` is called as work proceeds.
pub fn install(
    t: &Target,
    payload: &[Entry<'_>],
    opts: &Options,
    self_exe: Option<&Path>,
    mut progress: impl FnMut(f32, &str),
) -> Result<Manifest, String> {
    if payload.is_empty() {
        return Err("This installer contains no files (development build). Build it with scripts\\package.ps1.".into());
    }
    std::fs::create_dir_all(&opts.dir).map_err(|e| format!("cannot create {}: {e}", opts.dir.display()))?;
    let old = read_manifest(&opts.dir);
    let mut files = Vec::new();
    let total = payload.len() as f32 + 2.0;
    for (i, e) in payload.iter().enumerate() {
        progress(i as f32 / total, &format!("Copying {}", e.path));
        let dest = opts.dir.join(e.path.replace('/', "\\"));
        write_atomic(&dest, e.data)?;
        files.push(ManifestFile { path: e.path.clone(), sha256: sha256_hex(e.data), size: e.data.len() as u64 });
    }
    if let Some(exe) = self_exe {
        let dest = opts.dir.join(UNINSTALLER_EXE);
        if exe != dest {
            let data = std::fs::read(exe).map_err(|e| format!("cannot read installer: {e}"))?;
            write_atomic(&dest, &data)?;
            files.push(ManifestFile {
                path: UNINSTALLER_EXE.into(),
                sha256: sha256_hex(&data),
                size: data.len() as u64,
            });
        } else if let Some(m) = old.as_ref().and_then(|m| m.files.iter().find(|f| f.path == UNINSTALLER_EXE)) {
            files.push(m.clone());
        }
    }
    // Remove files from an older version that this version no longer ships.
    if let Some(old) = &old {
        for f in &old.files {
            if !files.iter().any(|n| n.path == f.path) {
                let _ = std::fs::remove_file(opts.dir.join(&f.path));
            }
        }
        if t.system {
            remove_shortcuts(&old.shortcuts);
        }
    }

    progress((total - 2.0) / total, "Registering");
    let mut shortcuts = Vec::new();
    if t.system {
        let exe = opts.dir.join(APP_EXE);
        if opts.start_menu_shortcut
            && let Some(p) = start_menu_dir().map(|d| d.join(format!("{APP_NAME}.lnk")))
        {
            create_shortcut(&p, &exe, &opts.dir)?;
            shortcuts.push(p);
        }
        if opts.desktop_shortcut
            && let Some(p) = desktop_dir().map(|d| d.join(format!("{APP_NAME}.lnk")))
        {
            create_shortcut(&p, &exe, &opts.dir)?;
            shortcuts.push(p);
        }
        if opts.associate_files {
            associate(t, &exe)?;
        } else {
            unassociate(t);
        }
    }
    let size_kb = files.iter().map(|f| f.size).sum::<u64>() / 1024;
    let manifest = Manifest { version: VERSION.into(), options: opts.clone(), files, shortcuts };
    write_atomic(&opts.dir.join(MANIFEST), &serde_json::to_vec_pretty(&manifest).unwrap())?;
    if t.system {
        register(t, opts, size_kb)?;
    }
    progress(1.0, "Done");
    Ok(manifest)
}

/// Files that are missing or differ from the manifest (what Repair would fix).
pub fn verify(dir: &Path) -> Result<Vec<String>, String> {
    let m = read_manifest(dir).ok_or("No install manifest found; reinstall Soundesti.")?;
    Ok(m.files
        .iter()
        .filter(|f| match std::fs::read(dir.join(&f.path)) {
            Ok(d) => sha256_hex(&d) != f.sha256,
            Err(_) => true,
        })
        .map(|f| f.path.clone())
        .collect())
}

/// Removes the installation. Returns true if the folder will be deleted after this
/// process exits (because the running uninstaller lives inside it).
pub fn uninstall(
    t: &Target,
    inst: &Installed,
    keep_user_data: bool,
    mut progress: impl FnMut(f32, &str),
) -> Result<bool, String> {
    let running = std::env::current_exe().ok();
    let files: Vec<String> = match &inst.manifest {
        Some(m) => m.files.iter().map(|f| f.path.clone()).collect(),
        None => vec![APP_EXE.into(), UNINSTALLER_EXE.into()],
    };
    let mut deferred = false;
    for (i, f) in files.iter().enumerate() {
        progress(i as f32 / (files.len() as f32 + 1.0), &format!("Removing {f}"));
        let p = inst.dir.join(f);
        if running.as_deref() == Some(p.as_path()) {
            deferred = true;
            continue;
        }
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                return Err(format!("{} is in use. Please close Soundesti and try again.", p.display()));
            }
            Err(e) => return Err(format!("cannot remove {}: {e}", p.display())),
        }
    }
    let _ = std::fs::remove_file(inst.dir.join(MANIFEST));
    if t.system {
        if let Some(m) = &inst.manifest {
            remove_shortcuts(&m.shortcuts);
        }
        unassociate(t);
        let _ = RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(&t.uninstall_key);
        if !keep_user_data && let Some(d) = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(APP_NAME)) {
            let _ = std::fs::remove_dir_all(d);
        }
    }
    remove_empty_dirs(&inst.dir);
    if deferred && t.system {
        schedule_delete(&inst.dir);
    }
    progress(1.0, "Removed");
    Ok(deferred)
}

fn remove_empty_dirs(dir: &Path) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            if e.path().is_dir() {
                remove_empty_dirs(&e.path());
            }
        }
    }
    let _ = std::fs::remove_dir(dir);
}

/// Deletes the running uninstaller a few seconds after we exit (it cannot delete itself),
/// then removes the install folder only if it is now empty. Never recursive: files the
/// user put in the folder (songs, presets) are left alone.
fn schedule_delete(dir: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let exe = dir.join(UNINSTALLER_EXE);
    // raw_arg: cmd.exe has its own quoting rules; Rust's normal argument escaping turns the
    // quotes around the paths into \" which cmd doesn't understand, so the delete silently failed.
    let cmd = format!("/C ping -n 4 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"", exe.display(), dir.display());
    let _ = std::process::Command::new("cmd").raw_arg(cmd).creation_flags(CREATE_NO_WINDOW).spawn();
}

fn register(t: &Target, opts: &Options, size_kb: u64) -> Result<(), String> {
    let (k, _) =
        RegKey::predef(HKEY_CURRENT_USER).create_subkey(&t.uninstall_key).map_err(|e| format!("registry: {e}"))?;
    let exe = opts.dir.join(APP_EXE);
    let uninst = opts.dir.join(UNINSTALLER_EXE);
    let set = |name: &str, v: String| k.set_value(name, &v).map_err(|e| format!("registry: {e}"));
    set("DisplayName", APP_NAME.into())?;
    set("DisplayVersion", VERSION.into())?;
    set("Publisher", "Soundesti project".into())?;
    set("InstallLocation", opts.dir.display().to_string())?;
    set("DisplayIcon", format!("{},0", exe.display()))?;
    set("UninstallString", format!("\"{}\" --uninstall", uninst.display()))?;
    set("ModifyPath", format!("\"{}\" --maintenance", uninst.display()))?;
    set("URLInfoAbout", "https://www.gnu.org/licenses/gpl-3.0.html".into())?;
    k.set_value("EstimatedSize", &(size_kb.min(u32::MAX as u64) as u32)).map_err(|e| format!("registry: {e}"))?;
    k.set_value("NoModify", &0u32).map_err(|e| format!("registry: {e}"))?;
    k.set_value("NoRepair", &0u32).map_err(|e| format!("registry: {e}"))?;
    Ok(())
}

fn associate(t: &Target, exe: &Path) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let err = |e: std::io::Error| format!("file association: {e}");
    let (ext, _) = hkcu.create_subkey(format!(r"{}\.soundesti", t.classes_key)).map_err(err)?;
    ext.set_value("", &"Soundesti.Project").map_err(err)?;
    let (prog, _) = hkcu.create_subkey(format!(r"{}\Soundesti.Project", t.classes_key)).map_err(err)?;
    prog.set_value("", &"Soundesti Project").map_err(err)?;
    let (icon, _) = prog.create_subkey("DefaultIcon").map_err(err)?;
    icon.set_value("", &format!("{},0", exe.display())).map_err(err)?;
    let (cmd, _) = prog.create_subkey(r"shell\open\command").map_err(err)?;
    cmd.set_value("", &format!("\"{}\" \"%1\"", exe.display())).map_err(err)?;
    notify_shell();
    Ok(())
}

fn unassociate(t: &Target) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let _ = hkcu.delete_subkey_all(format!(r"{}\.soundesti", t.classes_key));
    let _ = hkcu.delete_subkey_all(format!(r"{}\Soundesti.Project", t.classes_key));
    notify_shell();
}

fn notify_shell() {
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    // SAFETY: documented call with null item pointers for SHCNE_ASSOCCHANGED.
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
}

fn known_folder(name: &str) -> Option<PathBuf> {
    // [Environment]::GetFolderPath honours OneDrive / redirected folders.
    let out = hidden_powershell(&format!("[Environment]::GetFolderPath('{name}')")).ok()?;
    let s = out.trim();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

pub fn desktop_dir() -> Option<PathBuf> {
    known_folder("Desktop")
}

pub fn start_menu_dir() -> Option<PathBuf> {
    known_folder("Programs")
}

fn hidden_powershell(script: &str) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

fn ps_quote(p: &Path) -> String {
    format!("'{}'", p.display().to_string().replace('\'', "''"))
}

fn create_shortcut(lnk: &Path, target: &Path, workdir: &Path) -> Result<(), String> {
    let script = format!(
        "$s=(New-Object -ComObject WScript.Shell).CreateShortcut({}); $s.TargetPath={}; $s.WorkingDirectory={}; $s.Description='Soundesti DAW'; $s.Save()",
        ps_quote(lnk),
        ps_quote(target),
        ps_quote(workdir)
    );
    hidden_powershell(&script).map(|_| ()).map_err(|e| format!("cannot create shortcut {}: {e}", lnk.display()))
}

fn remove_shortcuts(list: &[PathBuf]) {
    for p in list {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_target() -> Target {
        Target {
            uninstall_key: r"Software\SoundestiInstallerTest\Uninstall".into(),
            classes_key: r"Software\SoundestiInstallerTest\Classes".into(),
            system: false,
        }
    }

    fn opts(name: &str) -> Options {
        let dir = std::env::temp_dir().join(format!("soundesti-inst-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Options { dir, desktop_shortcut: false, start_menu_shortcut: false, associate_files: false }
    }

    fn payload() -> Vec<Entry<'static>> {
        vec![
            Entry { path: APP_EXE.into(), data: b"fake exe v1" },
            Entry { path: "docs/readme.txt".into(), data: b"hi" },
        ]
    }

    #[test]
    fn install_verify_repair_uninstall() {
        let t = test_target();
        let o = opts("cycle");
        let mut steps = 0;
        let m = install(&t, &payload(), &o, None, |_, _| steps += 1).unwrap();
        assert!(steps >= 3);
        assert_eq!(m.files.len(), 2);
        assert_eq!(std::fs::read(o.dir.join(APP_EXE)).unwrap(), b"fake exe v1");
        assert!(verify(&o.dir).unwrap().is_empty());

        // Corrupt and delete files: verify reports them, repair (reinstall) fixes them.
        std::fs::write(o.dir.join(APP_EXE), b"corrupted").unwrap();
        std::fs::remove_file(o.dir.join("docs").join("readme.txt")).unwrap();
        let mut bad = verify(&o.dir).unwrap();
        bad.sort();
        assert_eq!(bad, vec!["docs/readme.txt".to_string(), APP_EXE.to_string()]);
        install(&t, &payload(), &o, None, |_, _| {}).unwrap();
        assert!(verify(&o.dir).unwrap().is_empty());

        // User files inside the folder survive uninstall; installer files do not.
        std::fs::write(o.dir.join("my-song.soundesti"), b"user data").unwrap();
        let inst = Installed { dir: o.dir.clone(), version: VERSION.into(), manifest: read_manifest(&o.dir) };
        uninstall(&t, &inst, true, |_, _| {}).unwrap();
        assert!(!o.dir.join(APP_EXE).exists());
        assert!(!o.dir.join(MANIFEST).exists());
        assert!(o.dir.join("my-song.soundesti").exists());
        std::fs::remove_dir_all(&o.dir).ok();
    }

    #[test]
    fn upgrade_removes_files_no_longer_shipped() {
        let t = test_target();
        let o = opts("upgrade");
        install(&t, &payload(), &o, None, |_, _| {}).unwrap();
        let v2 = vec![Entry { path: APP_EXE.into(), data: b"fake exe v2" }];
        install(&t, &v2, &o, None, |_, _| {}).unwrap();
        assert!(!o.dir.join("docs").join("readme.txt").exists());
        assert_eq!(std::fs::read(o.dir.join(APP_EXE)).unwrap(), b"fake exe v2");
        std::fs::remove_dir_all(&o.dir).ok();
    }

    #[test]
    fn empty_payload_is_refused() {
        let o = opts("empty");
        assert!(install(&test_target(), &[], &o, None, |_, _| {}).unwrap_err().contains("development build"));
    }

    #[test]
    fn registry_roundtrip_in_test_key() {
        let mut t = test_target();
        let o = opts("reg");
        std::fs::create_dir_all(&o.dir).unwrap();
        register(&t, &o, 123).unwrap();
        let k = RegKey::predef(HKEY_CURRENT_USER).open_subkey(&t.uninstall_key).unwrap();
        let loc: String = k.get_value("InstallLocation").unwrap();
        assert_eq!(PathBuf::from(loc), o.dir);
        let size: u32 = k.get_value("EstimatedSize").unwrap();
        assert_eq!(size, 123);
        t.system = true;
        associate(&t, &o.dir.join(APP_EXE)).unwrap();
        let cmd: String = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(format!(r"{}\Soundesti.Project\shell\open\command", t.classes_key))
            .unwrap()
            .get_value("")
            .unwrap();
        assert!(cmd.ends_with("\"%1\""));
        unassociate(&t);
        assert!(RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(r"{}\.soundesti", t.classes_key)).is_err());
        RegKey::predef(HKEY_CURRENT_USER).delete_subkey_all(r"Software\SoundestiInstallerTest").unwrap();
        std::fs::remove_dir_all(&o.dir).ok();
    }
}
