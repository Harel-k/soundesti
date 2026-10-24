//! Save / open / recent files / autosave / WAV export jobs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use soundesti_audio::Command as EngineCmd;
use soundesti_export::{ExportOptions, ExportReport};
use soundesti_project::{FILE_EXTENSION, Project};

use crate::app::{AfterConfirm, App, Dialog};

pub struct ExportJob {
    pub progress: Arc<AtomicU32>,
    pub cancel: Arc<AtomicBool>,
    rx: mpsc::Receiver<Result<ExportReport, String>>,
    pub path: PathBuf,
    pub started: Instant,
}

impl ExportJob {
    pub fn fraction(&self) -> f32 {
        f32::from_bits(self.progress.load(Ordering::Relaxed))
    }
}

const AUTOSAVE_EVERY: Duration = Duration::from_secs(120);

pub fn autosave_path() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Soundesti").join("autosave.soundesti"))
}

fn with_extension(mut p: PathBuf, ext: &str) -> PathBuf {
    if p.extension().is_none_or(|e| !e.eq_ignore_ascii_case(ext)) {
        let mut s = p.into_os_string();
        s.push(".");
        s.push(ext);
        p = PathBuf::from(s);
    }
    p
}

/// `song.soundesti` → `song_2.soundesti`, `song_2` → `song_3`, skipping existing files.
pub fn next_version_path(path: &Path) -> PathBuf {
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let (base, mut n) = match stem.rsplit_once('_') {
        Some((b, num)) if num.parse::<u32>().is_ok() => (b.to_string(), num.parse::<u32>().unwrap()),
        _ => (stem.clone(), 1),
    };
    loop {
        n += 1;
        let candidate = path.with_file_name(format!("{base}_{n}.{FILE_EXTENSION}"));
        if !candidate.exists() {
            return candidate;
        }
    }
}

impl App {
    pub fn save(&mut self, choose: bool) {
        let path = match (&self.path, choose) {
            (Some(p), false) => Some(p.clone()),
            _ => rfd::FileDialog::new()
                .add_filter("Soundesti project", &[FILE_EXTENSION])
                .set_file_name(format!("{}.{FILE_EXTENSION}", self.title_or_untitled()))
                .save_file()
                .map(|p| with_extension(p, FILE_EXTENSION)),
        };
        if let Some(p) = path {
            self.save_to(p);
        }
    }

    fn title_or_untitled(&self) -> String {
        if self.project.meta.title.is_empty() { "Untitled".into() } else { self.project.meta.title.clone() }
    }

    fn save_to(&mut self, path: PathBuf) -> bool {
        match soundesti_project::save(&self.project, &path) {
            Ok(()) => {
                self.saved_revision = self.revision;
                self.set_status(format!("Saved {}", path.display()));
                self.add_recent(path.clone());
                self.path = Some(path);
                true
            }
            Err(e) => {
                self.dialog = Some(Dialog::Message { title: "Save failed".into(), text: e.to_string() });
                false
            }
        }
    }

    pub fn save_new_version(&mut self) {
        match self.path.clone() {
            Some(p) => {
                let next = next_version_path(&p);
                self.save_to(next);
            }
            None => self.save(true),
        }
    }

    pub fn add_recent(&mut self, p: PathBuf) {
        if autosave_path().as_deref() == Some(p.as_path()) {
            return;
        }
        self.recent.retain(|r| r != &p);
        self.recent.insert(0, p);
        self.recent.truncate(10);
    }

    /// Asks to save unsaved changes before `after`, then performs it.
    pub fn request(&mut self, after: AfterConfirm) {
        if self.dirty() {
            self.dialog = Some(Dialog::ConfirmDiscard { then: after });
        } else {
            self.after = Some(after);
        }
    }

    pub fn perform(&mut self, ctx: &egui::Context, after: AfterConfirm) {
        match after {
            AfterConfirm::New => {
                self.session.send(EngineCmd::Stop);
                self.replace_project(Project::new(), None);
                self.set_status("New project.");
            }
            AfterConfirm::Open(path) => {
                let path = path
                    .or_else(|| rfd::FileDialog::new().add_filter("Soundesti project", &[FILE_EXTENSION]).pick_file());
                let Some(path) = path else { return };
                match soundesti_project::load(&path) {
                    Ok(p) => {
                        self.session.send(EngineCmd::Stop);
                        self.replace_project(p, Some(path.clone()));
                        self.add_recent(path.clone());
                        self.set_status(format!("Opened {}", path.display()));
                    }
                    Err(e) => {
                        self.dialog =
                            Some(Dialog::Message { title: "Could not open project".into(), text: e.to_string() })
                    }
                }
            }
            AfterConfirm::Quit => {
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn replace_project(&mut self, project: Project, path: Option<PathBuf>) {
        let is_autosave = path.as_deref().is_some() && path.as_deref() == autosave_path().as_deref();
        self.project = project;
        self.history.clear();
        self.revision += 1;
        self.saved_revision = self.revision;
        // Opening the autosave must not make Ctrl+S overwrite it silently.
        self.path = if is_autosave { None } else { path };
        self.session.solo.clear();
        self.mixer_solo = None;
        self.selected_channels.clear();
        self.selected_channel = None;
        self.win.instruments.clear();
        self.fix_selection();
        self.needs_sync = true;
    }

    pub fn autosave_tick(&mut self) {
        if !self.dirty() || self.last_autosave.elapsed() < AUTOSAVE_EVERY {
            return;
        }
        self.last_autosave = Instant::now();
        let Some(path) = autosave_path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match soundesti_project::save(&self.project, &path) {
            Ok(()) => log::info!("autosaved to {}", path.display()),
            Err(e) => self.set_status(format!("Autosave failed: {e}")),
        }
    }

    pub fn start_export(&mut self) {
        if self.export_job.is_some() {
            self.set_status("An export is already running.");
            return;
        }
        self.win.export = true;
    }

    pub fn begin_export(&mut self, opts: ExportOptions) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("WAV audio", &["wav"])
            .set_file_name(format!("{}.wav", self.title_or_untitled()))
            .save_file()
            .map(|p| with_extension(p, "wav"))
        else {
            return;
        };
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let project = self.project.clone();
        let (p2, c2, path2) = (progress.clone(), cancel.clone(), path.clone());
        std::thread::Builder::new()
            .name("soundesti-export".into())
            .spawn(move || {
                let res = soundesti_export::export_wav(&project, &path2, &opts, |f| {
                    p2.store(f.to_bits(), Ordering::Relaxed);
                    !c2.load(Ordering::Relaxed)
                })
                .map_err(|e| e.to_string());
                let _ = tx.send(res);
            })
            .expect("spawn export thread");
        self.export_job = Some(ExportJob { progress, cancel, rx, path, started: Instant::now() });
        self.win.export = false;
    }

    pub fn poll_export(&mut self) {
        let Some(job) = &self.export_job else { return };
        match job.rx.try_recv() {
            Ok(Ok(r)) => {
                let msg = format!(
                    "Exported {} ({:.1} s, peak {:.1} dBFS) in {:.1} s",
                    job.path.display(),
                    r.seconds,
                    20.0 * r.peak.max(1e-9).log10(),
                    job.started.elapsed().as_secs_f32()
                );
                self.set_status(msg);
                self.export_job = None;
            }
            Ok(Err(e)) => {
                self.set_status(format!("Export stopped: {e}"));
                self.export_job = None;
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.set_status("Export failed unexpectedly.");
                self.export_job = None;
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_paths() {
        let dir = std::env::temp_dir().join(format!("soundesti-ver-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("beat.soundesti");
        assert_eq!(next_version_path(&p).file_name().unwrap(), "beat_2.soundesti");
        std::fs::write(dir.join("beat_2.soundesti"), b"x").unwrap();
        assert_eq!(next_version_path(&p).file_name().unwrap(), "beat_3.soundesti");
        assert_eq!(next_version_path(&dir.join("beat_7.soundesti")).file_name().unwrap(), "beat_8.soundesti");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn extension_is_added_once() {
        assert_eq!(with_extension(PathBuf::from("a"), "wav"), PathBuf::from("a.wav"));
        assert_eq!(with_extension(PathBuf::from("a.WAV"), "wav"), PathBuf::from("a.WAV"));
        assert_eq!(with_extension(PathBuf::from("a.mid"), "wav"), PathBuf::from("a.mid.wav"));
    }
}
