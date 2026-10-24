//! In-app auto-update: background check, download with SHA-256 verification, then hand
//! off to the installer (`--update`) after the user agrees and unsaved work is handled.

use std::path::PathBuf;
use std::sync::mpsc;

use soundesti_updater::UpdateInfo;

use crate::app::{AfterConfirm, App, Dialog};

pub enum UpdateState {
    Idle,
    Checking { rx: mpsc::Receiver<Result<Option<UpdateInfo>, String>>, manual: bool },
    Available(UpdateInfo),
    Downloading { rx: mpsc::Receiver<Result<PathBuf, String>> },
    Ready(PathBuf),
}

pub const AUTO_CHECK_KEY: &str = "soundesti.auto_update";

impl App {
    /// Starts a background check. `manual` reports "up to date" / errors in the hint bar.
    pub fn check_for_updates(&mut self, manual: bool) {
        if !matches!(self.update, UpdateState::Idle | UpdateState::Available(_)) {
            return;
        }
        let Some(feed) = soundesti_updater::feed_url() else {
            if manual {
                self.set_status("Online updates aren't available yet: no release server is configured. Run the Installer Wizard to reinstall/update offline.");
            }
            return;
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let r = soundesti_updater::check(&feed, env!("CARGO_PKG_VERSION"), false).map_err(|e| e.to_string());
            let _ = tx.send(r);
        });
        self.update = UpdateState::Checking { rx, manual };
        if manual {
            self.set_status("Checking for updates…");
        }
    }

    pub fn poll_updates(&mut self) {
        match &self.update {
            UpdateState::Checking { rx, manual } => {
                let manual = *manual;
                if let Ok(r) = rx.try_recv() {
                    self.update = UpdateState::Idle;
                    match r {
                        Ok(Some(info)) => {
                            self.set_status(format!(
                                "Soundesti {} is available (Help › Install update).",
                                info.version
                            ));
                            self.dialog = Some(Dialog::Message {
                                title: "Update available".into(),
                                text: format!(
                                    "Soundesti {} is available.\n\n{}\n\nUse Help › Install update to download it.",
                                    info.version, info.notes
                                ),
                            });
                            self.update = UpdateState::Available(info);
                        }
                        Ok(None) if manual => self.set_status("Soundesti is up to date."),
                        Err(e) if manual => self.set_status(format!("Update check failed: {e}")),
                        _ => {}
                    }
                }
            }
            UpdateState::Downloading { rx } => {
                if let Ok(r) = rx.try_recv() {
                    match r {
                        Ok(path) => {
                            self.set_status("Update downloaded and verified. Help › Restart to update.");
                            self.update = UpdateState::Ready(path);
                        }
                        Err(e) => {
                            self.set_status(format!("Update download failed: {e}"));
                            self.update = UpdateState::Idle;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    pub fn download_update(&mut self) {
        let UpdateState::Available(info) = &self.update else { return };
        let info = info.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let dir = std::env::temp_dir().join("Soundesti-update");
            let _ = tx.send(soundesti_updater::download(&info, &dir, |_| true).map_err(|e| e.to_string()));
        });
        self.set_status("Downloading update…");
        self.update = UpdateState::Downloading { rx };
    }

    /// Asks to save unsaved work, then quits and launches the verified installer in update
    /// mode. The installer is only started once quitting is confirmed.
    pub fn restart_to_update(&mut self) {
        let UpdateState::Ready(path) = &self.update else { return };
        let path = path.clone();
        self.request(AfterConfirm::QuitAndUpdate(path));
    }

    pub fn update_menu(&mut self, ui: &mut egui::Ui) {
        if ui.button("Check for updates…").clicked() {
            self.check_for_updates(true);
            ui.close();
        }
        let label = match &self.update {
            UpdateState::Available(i) => Some(format!("Install update {}…", i.version)),
            UpdateState::Ready(_) => Some("Restart to update".to_string()),
            _ => None,
        };
        if let Some(l) = label
            && ui.button(l).clicked()
        {
            if matches!(self.update, UpdateState::Ready(_)) {
                self.restart_to_update();
            } else {
                self.download_update();
            }
            ui.close();
        }
        let mut auto = self.auto_update;
        if ui.checkbox(&mut auto, "Check for updates automatically").changed() {
            self.auto_update = auto;
        }
    }
}
