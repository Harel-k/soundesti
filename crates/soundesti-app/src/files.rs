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
    rx: mpsc::Receiver<Result<String, String>>,
    pub path: PathBuf,
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

/// `song.soundesti` â†’ `song_2.soundesti`, `song_2` â†’ `song_3`, skipping existing files.
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

/// (name, instrument type, General MIDI keys, instrument params) for MIDI drum import.
type DrumGroup = (&'static str, &'static str, &'static [u8], &'static [(&'static str, f32)]);

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

    pub fn save_to(&mut self, path: PathBuf) -> bool {
        self.session.capture_plugin_states(&mut self.project);
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
                let path = path.or_else(|| {
                    rfd::FileDialog::new()
                        .add_filter("Projects (Soundesti, FL Studio, MIDI)", &[FILE_EXTENSION, "flp", "mid", "midi"])
                        .add_filter("Soundesti project", &[FILE_EXTENSION])
                        .add_filter("FL Studio project", &["flp"])
                        .pick_file()
                });
                let Some(path) = path else { return };
                if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("flp")) {
                    self.import_flp(&path);
                    return;
                }
                if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("mid") || e.eq_ignore_ascii_case("midi")) {
                    self.session.send(EngineCmd::Stop);
                    self.replace_project(Project::empty(), None);
                    self.import_midi(&path);
                    self.saved_revision = self.revision.wrapping_sub(1);
                    return;
                }
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
            AfterConfirm::ImportFlp => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("FL Studio project", &["flp"])
                    .set_title("Import FL Studio project")
                    .pick_file()
                {
                    self.import_flp(&path);
                }
            }
            AfterConfirm::Quit => {
                self.quit_confirmed = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            AfterConfirm::QuitAndUpdate(installer) => {
                match std::process::Command::new(&installer).arg("--update").spawn() {
                    Ok(_) => {
                        self.quit_confirmed = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(e) => self.set_status(format!("Could not start the updater: {e}")),
                }
            }
        }
    }

    /// Adds a MIDI file to the project: one channel per part, in a new pattern named after
    /// the file. General MIDI drum parts become Soundesti drum synths split by sound.
    pub fn import_midi(&mut self, path: &std::path::Path) {
        let song = match std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|d| soundesti_midi_file::read(&d).map_err(|e| e.to_string()))
        {
            Ok(s) => s,
            Err(e) => {
                self.dialog = Some(Dialog::Message {
                    title: "Could not import MIDI file".into(),
                    text: format!("{}: {e}", path.display()),
                });
                return;
            }
        };
        if song.parts.iter().all(|p| p.notes.is_empty()) {
            self.set_status("That MIDI file has no notes.");
            return;
        }
        self.checkpoint("Import MIDI file");
        if let Some(bpm) = song.bpm {
            self.project.bpm = bpm.clamp(10.0, 522.0);
        }
        if let Some(ts) = song.time_signature {
            self.project.time_signature = ts;
        }
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "MIDI".into());
        // Reuse an empty current pattern, else start a new one.
        let pid =
            if self.project.current().is_empty() { self.project.current_pattern } else { self.project.add_pattern() };
        if let Some(p) = self.project.pattern_mut(pid) {
            p.name = stem.clone();
        }
        let mut channels = 0;
        let drum_word = |s: &str| ["drum", "fpc", "perc", "kit", "beat"].iter().any(|w| s.to_lowercase().contains(w));
        for part in song.parts.iter().filter(|p| !p.notes.is_empty()) {
            // GM drum channel, or a part/file that says it is drums (FPC loops use channel 1).
            if part.channel == 9 || drum_word(&part.name) || drum_word(&stem) {
                // GM drum map: 35/36 kick, 37-40 snare/clap, 42/44/46 hats, cymbals 49-59.
                let groups: [DrumGroup; 4] = [
                    ("Kick", "soundesti.kickr", &[35, 36], &[]),
                    ("Snare", "soundesti.clapper", &[37, 38, 40], &[("mode.claps", 0.0)]),
                    ("Clap", "soundesti.clapper", &[39], &[("mode.claps", 3.0)]),
                    ("Hats", "soundesti.hatz", &[42, 44, 46, 49, 51, 52, 53, 55, 57, 59], &[]),
                ];
                for (name, ty, keys, params) in groups {
                    let notes: Vec<_> = part.notes.iter().filter(|n| keys.contains(&n.key)).collect();
                    if notes.is_empty() {
                        continue;
                    }
                    let id = self.project.add_channel(format!("{} {name}", part.name), ty);
                    if let Some(c) = self.project.channel_mut(id) {
                        for (k, v) in params {
                            c.instrument.params.insert(k.to_string(), *v);
                        }
                    }
                    let open = [46u8, 49, 51, 52, 53, 55, 57, 59];
                    let list = self.project.pattern_mut(pid).unwrap().notes_mut(id);
                    for n in notes {
                        let mut note = soundesti_project::Note::new(
                            n.start,
                            n.length,
                            if open.contains(&n.key) { 72 } else { 60 },
                        );
                        note.velocity = n.velocity;
                        list.push(note);
                    }
                    channels += 1;
                }
            } else {
                let id = self.project.add_channel(&part.name, "soundesti.trident");
                let list = self.project.pattern_mut(pid).unwrap().notes_mut(id);
                for n in &part.notes {
                    let mut note = soundesti_project::Note::new(n.start, n.length, n.key);
                    note.velocity = n.velocity;
                    list.push(note);
                }
                channels += 1;
            }
        }
        if let Some(p) = self.project.pattern_mut(pid) {
            for pc in &mut p.channels {
                pc.notes.sort_by_key(|n| (n.start, n.key));
            }
        }
        self.project.current_pattern = pid;
        self.needs_sync = true;
        self.set_status(format!("Imported {} ({channels} channels) into pattern \"{stem}\".", path.display()));
    }

    /// Piano roll Ctrl+M: every note of a MIDI file into the selected channel.
    pub fn import_midi_to_channel(&mut self) {
        let Some(ch) = self.selected_channel else { return };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MIDI file", &["mid", "midi"])
            .set_title("Import MIDI into this channel")
            .pick_file()
        else {
            return;
        };
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|d| soundesti_midi_file::read(&d).map_err(|e| e.to_string()))
        {
            Ok(song) => {
                self.checkpoint("Import MIDI into channel");
                let list = self.project.current_mut().notes_mut(ch);
                for n in song.parts.iter().flat_map(|p| &p.notes) {
                    let mut note = soundesti_project::Note::new(n.start, n.length, n.key);
                    note.velocity = n.velocity;
                    list.push(note);
                }
                list.sort_by_key(|n| (n.start, n.key));
                self.needs_sync = true;
                self.set_status(format!("Imported notes from {}.", path.display()));
            }
            Err(e) => self.set_status(format!("Could not import MIDI: {e}")),
        }
    }

    /// File > Export MIDI (Ctrl+Shift+M): the song if the Playlist has clips, else the pattern.
    pub fn export_midi(&mut self) {
        let song = self.project.song_mode && !self.project.playlist.clips.is_empty();
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MIDI file", &["mid"])
            .set_file_name(format!("{}.mid", self.title_or_untitled()))
            .save_file()
            .map(|p| with_extension(p, "mid"))
        else {
            return;
        };
        match std::fs::write(&path, soundesti_midi_file::write(&self.project, song)) {
            Ok(()) => self.set_status(format!(
                "Exported {} as MIDI ({}).",
                path.display(),
                if song { "song" } else { "current pattern" }
            )),
            Err(e) => self.set_status(format!("Could not write {}: {e}", path.display())),
        }
    }

    /// Imports an FL Studio project as a new, unsaved Soundesti project and shows the report.
    pub fn import_flp(&mut self, path: &std::path::Path) {
        match soundesti_flp::import_file(path) {
            Ok(imp) => {
                self.session.send(EngineCmd::Stop);
                let mut project = imp.project;
                if project.meta.title.is_empty() {
                    project.meta.title = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                }
                let summary = format!(
                    "{} channels, {} patterns, {} Playlist clips at {:.1} BPM.",
                    project.channels.len(),
                    project.patterns.len(),
                    project.playlist.clips.len(),
                    project.bpm
                );
                self.replace_project(project, None);
                // Not saved yet: Ctrl+S asks where to save the .soundesti file.
                self.saved_revision = self.revision.wrapping_sub(1);
                let version = if imp.fl_version.is_empty() { String::new() } else { format!(" {}", imp.fl_version) };
                let mut text = format!("Imported from FL Studio{version}: {summary}\n");
                for r in &imp.report {
                    text.push_str(&format!("\n• {r}"));
                }
                text.push_str("\n\nSave it (Ctrl+S) to keep it as a Soundesti project. The .flp file was not changed.");
                self.dialog = Some(Dialog::Message { title: "FL Studio project imported".into(), text });
                self.set_status(format!("Imported {}", path.display()));
            }
            Err(e) => {
                self.dialog = Some(Dialog::Message {
                    title: "Could not import FL Studio project".into(),
                    text: format!("{}: {e}", path.display()),
                });
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
        self.session.capture_plugin_states(&mut self.project);
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
        let ext = opts.format.extension();
        let label = match opts.format {
            soundesti_export::Format::Wav => "WAV audio",
            soundesti_export::Format::Flac => "FLAC audio",
            soundesti_export::Format::Mp3 { .. } => "MP3 audio",
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter(label, &[ext])
            .set_file_name(format!("{}.{ext}", self.title_or_untitled()))
            .save_file()
            .map(|p| with_extension(p, ext))
        else {
            return;
        };
        let progress = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        // Plugins render with the settings you hear right now.
        self.session.capture_plugin_states(&mut self.project);
        let project = self.project.clone();
        let stems = self.export_stems;
        let (p2, c2, path2) = (progress.clone(), cancel.clone(), path.clone());
        std::thread::Builder::new()
            .name("soundesti-export".into())
            .spawn(move || {
                let started = Instant::now();
                let tick = |f: f32| {
                    p2.store(f.to_bits(), Ordering::Relaxed);
                    !c2.load(Ordering::Relaxed)
                };
                let res = if stems {
                    soundesti_export::export_stems(&project, &path2, &opts, tick).map(|files| {
                        let dir = path2.parent().map(|d| d.display().to_string()).unwrap_or_default();
                        format!("Exported {} stems to {dir} in {:.1} s", files.len(), started.elapsed().as_secs_f32())
                    })
                } else {
                    soundesti_export::export(&project, &path2, &opts, tick).map(|r: ExportReport| {
                        format!(
                            "Exported {} ({:.1} s, peak {:.1} dBFS) in {:.1} s",
                            path2.display(),
                            r.seconds,
                            20.0 * r.peak.max(1e-9).log10(),
                            started.elapsed().as_secs_f32()
                        )
                    })
                };
                let _ = tx.send(res.map_err(|e| e.to_string()));
            })
            .expect("spawn export thread");
        self.export_job = Some(ExportJob { progress, cancel, rx, path });
        self.win.export = false;
    }

    pub fn poll_export(&mut self) {
        let Some(job) = &self.export_job else { return };
        match job.rx.try_recv() {
            Ok(Ok(msg)) => {
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
