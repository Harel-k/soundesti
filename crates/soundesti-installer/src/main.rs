//! Soundesti Installer Wizard.
//!
//! - Not installed: Welcome → License → Options → Installing → Finish.
//! - Installed: maintenance menu — Repair, Force update (offline reinstall or online
//!   download), Modify, Uninstall.
//!
//! Command line (also used by Apps & features and the in-app updater):
//! `--uninstall`, `--maintenance`, `--update` (reinstall over the existing install, then
//! launch), and headless `--silent [--dir PATH] [--no-desktop] [--no-startmenu]
//! [--no-assoc]`, `--silent-uninstall [--keep-data]`, `--verify`.
//! Headless modes report through the exit code (0 = success) and `%TEMP%\soundesti-setup.log`.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod payload;

use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use egui::{Color32, RichText};

use crate::core::{APP_EXE, APP_NAME, Installed, Options, Target, VERSION};

static PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.bin"));
const LICENSE: &str = include_str!("../../../LICENSE.txt");

const ACCENT: Color32 = Color32::from_rgb(0x2e, 0xd3, 0xb7);
const PANEL: Color32 = Color32::from_rgb(0x23, 0x29, 0x2f);
const DIM: Color32 = Color32::from_rgb(0x8a, 0x96, 0xa2);
const WARN: Color32 = Color32::from_rgb(0xff, 0xa0, 0x40);

fn entries() -> Result<Vec<payload::Entry<'static>>, String> {
    payload::parse(PAYLOAD)
}

fn log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) =
        std::fs::OpenOptions::new().create(true).append(true).open(std::env::temp_dir().join("soundesti-setup.log"))
    {
        let _ = writeln!(f, "{msg}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let has = |f: &str| args.iter().any(|a| a == f);
    let value = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();

    if has("--silent") || has("--silent-uninstall") || has("--verify") {
        std::process::exit(headless(&args, has("--silent-uninstall"), has("--verify"), value("--dir")));
    }

    let start = if has("--uninstall") {
        Start::Uninstall
    } else if has("--update") {
        Start::Update
    } else if has("--maintenance") {
        Start::Maintenance
    } else {
        Start::Auto
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("{APP_NAME} Setup {VERSION}"))
            .with_inner_size([760.0, 500.0])
            .with_resizable(false)
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../../../assets/icons/soundesti-256.png"))
                    .expect("bundled icon is a valid PNG"),
            ),
        ..Default::default()
    };
    let _ = eframe::run_native("Soundesti Setup", options, Box::new(move |cc| Ok(Box::new(Wizard::new(cc, start)))));
}

fn headless(args: &[String], uninstall: bool, verify: bool, dir: Option<String>) -> i32 {
    let has = |f: &str| args.iter().any(|a| a == f);
    let t = Target::default();
    let result: Result<String, String> = if verify {
        match core::detect(&t) {
            Some(i) => core::verify(&i.dir)
                .map(|bad| if bad.is_empty() { "all files OK".into() } else { format!("damaged: {}", bad.join(", ")) }),
            None => Err("not installed".into()),
        }
    } else if uninstall {
        match core::detect(&t) {
            Some(i) => core::uninstall(&t, &i, has("--keep-data"), |_, _| {})
                .map(|_| format!("uninstalled from {}", i.dir.display())),
            None => Err("not installed".into()),
        }
    } else {
        let mut o =
            core::detect(&t).and_then(|i| i.manifest.map(|m| m.options)).unwrap_or_else(Options::default_for_user);
        if let Some(d) = dir {
            o.dir = PathBuf::from(d);
        }
        if has("--no-desktop") {
            o.desktop_shortcut = false;
        }
        if has("--no-startmenu") {
            o.start_menu_shortcut = false;
        }
        if has("--no-assoc") {
            o.associate_files = false;
        }
        entries().and_then(|e| {
            let me = std::env::current_exe().ok();
            core::install(&t, &e, &o, me.as_deref(), |_, _| {})
                .map(|m| format!("installed {} files to {}", m.files.len(), o.dir.display()))
        })
    };
    match result {
        Ok(m) => {
            log(&format!("OK: {m}"));
            0
        }
        Err(e) => {
            log(&format!("ERROR: {e}"));
            1
        }
    }
}

#[derive(Clone, Copy)]
enum Start {
    Auto,
    Maintenance,
    Uninstall,
    Update,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Welcome,
    License,
    Options,
    Working,
    Done,
    Maintenance,
    ConfirmUninstall,
    Error,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Install,
    Repair,
    Update,
    Modify,
    Uninstall,
}

struct Job {
    progress: Arc<Mutex<(f32, String)>>,
    rx: mpsc::Receiver<Result<String, String>>,
}

struct Wizard {
    page: Page,
    opts: Options,
    accepted: bool,
    installed: Option<Installed>,
    job: Option<Job>,
    action: Action,
    message: String,
    launch_after: bool,
    keep_data: bool,
    online: Option<mpsc::Receiver<Result<String, String>>>,
    online_status: String,
    close_after_online: bool,
}

impl Wizard {
    fn new(cc: &eframe::CreationContext<'_>, start: Start) -> Self {
        let mut v = egui::Visuals::dark();
        v.panel_fill = Color32::from_rgb(0x2a, 0x31, 0x38);
        v.selection.bg_fill = Color32::from_rgb(0x1e, 0x7f, 0x70);
        cc.egui_ctx.set_visuals(v);
        let installed = core::detect(&Target::default());
        let opts = installed
            .as_ref()
            .and_then(|i| i.manifest.as_ref().map(|m| m.options.clone()))
            .unwrap_or_else(Options::default_for_user);
        let mut w = Self {
            page: Page::Welcome,
            opts,
            accepted: false,
            installed,
            job: None,
            action: Action::Install,
            message: String::new(),
            launch_after: true,
            keep_data: true,
            online: None,
            online_status: String::new(),
            close_after_online: false,
        };
        w.page = match (start, w.installed.is_some()) {
            (Start::Uninstall, true) => Page::ConfirmUninstall,
            (Start::Update, true) => {
                w.start_job(Action::Update);
                Page::Working
            }
            (_, true) => Page::Maintenance,
            (Start::Uninstall, false) => {
                w.message = format!("{APP_NAME} is not installed.");
                Page::Error
            }
            _ => Page::Welcome,
        };
        w
    }

    fn start_job(&mut self, action: Action) {
        self.action = action;
        let progress = Arc::new(Mutex::new((0.0f32, String::from("Starting…"))));
        let (tx, rx) = mpsc::channel();
        let p = progress.clone();
        let opts = self.opts.clone();
        let installed = self.installed.clone();
        let keep = self.keep_data;
        std::thread::spawn(move || {
            let t = Target::default();
            let report = |f: f32, m: &str| *p.lock().unwrap() = (f, m.to_string());
            let res = match action {
                Action::Uninstall => match installed {
                    Some(i) => core::uninstall(&t, &i, keep, report).map(|deferred| {
                        if deferred {
                            "Soundesti was removed. The folder will disappear when this window closes.".into()
                        } else {
                            "Soundesti was removed from this computer.".into()
                        }
                    }),
                    None => Err("Soundesti is not installed.".into()),
                },
                _ => entries().and_then(|e| {
                    let me = std::env::current_exe().ok();
                    core::install(&t, &e, &opts, me.as_deref(), report).map(|m| {
                        let verb = match action {
                            Action::Repair => "repaired",
                            Action::Update => "updated",
                            Action::Modify => "modified",
                            _ => "installed",
                        };
                        format!(
                            "Soundesti {} was {verb} ({} files) in {}.",
                            m.version,
                            m.files.len(),
                            opts.dir.display()
                        )
                    })
                }),
            };
            log(&format!("{:?}", res));
            let _ = tx.send(res);
        });
        self.job = Some(Job { progress, rx });
        self.page = Page::Working;
    }

    fn start_online_update(&mut self) {
        let Some(feed) = soundesti_updater::feed_url() else {
            self.online_status = "Online updates aren't available yet: no release server is configured. Use \"Reinstall this version\" to force an offline update.".into();
            return;
        };
        let (tx, rx) = mpsc::channel();
        self.online_status = "Checking for updates…".into();
        std::thread::spawn(move || {
            let res = (|| -> Result<String, String> {
                let info = soundesti_updater::check(&feed, VERSION, true)
                    .map_err(|e| e.to_string())?
                    .ok_or("No update available.")?;
                let dir = std::env::temp_dir().join("Soundesti-update");
                let exe = soundesti_updater::download(&info, &dir, |_| true).map_err(|e| e.to_string())?;
                std::process::Command::new(&exe).arg("--update").spawn().map_err(|e| e.to_string())?;
                Ok(format!("Starting the {} installer…", info.version))
            })();
            let _ = tx.send(res);
        });
        self.online = Some(rx);
    }

    fn header(&self, ui: &mut egui::Ui, title: &str, subtitle: &str) {
        ui.label(RichText::new(title).size(22.0).strong());
        ui.label(RichText::new(subtitle).color(DIM));
        ui.add_space(12.0);
    }

    fn sidebar(&self, ui: &mut egui::Ui) {
        ui.add_space(18.0);
        ui.label(RichText::new("SOUNDESTI").size(24.0).strong().color(ACCENT));
        ui.label(RichText::new(format!("version {VERSION}")).color(DIM).small());
        ui.add_space(24.0);
        let maintenance = self.installed.is_some() && !matches!(self.page, Page::Welcome | Page::License);
        let steps: &[(Page, &str)] = if maintenance {
            &[(Page::Maintenance, "Choose action"), (Page::Working, "Working"), (Page::Done, "Finished")]
        } else {
            &[
                (Page::Welcome, "Welcome"),
                (Page::License, "License"),
                (Page::Options, "Options"),
                (Page::Working, "Installing"),
                (Page::Done, "Finished"),
            ]
        };
        for (p, name) in steps {
            let active = *p == self.page;
            ui.label(RichText::new(format!("{} {name}", if active { "▶" } else { "  " })).color(if active {
                ACCENT
            } else {
                DIM
            }));
        }
        ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
            ui.label(RichText::new("Free forever · no telemetry").color(DIM).small());
        });
    }

    fn content(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        match self.page {
            Page::Welcome => {
                self.header(ui, &format!("Welcome to {APP_NAME}"), "This wizard installs Soundesti on your computer.");
                ui.label("Soundesti is a free digital audio workstation with an FL Studio-style workflow.");
                ui.label("No administrator rights are needed; it installs for your user account.");
                if PAYLOAD.is_empty() {
                    ui.add_space(8.0);
                    ui.colored_label(WARN, "This is a development build of the installer with no files inside. Build it with scripts\\package.ps1.");
                }
            }
            Page::License => {
                self.header(ui, "License agreement", "Please review the license terms.");
                // LICENSE.txt is hard-wrapped; reflow paragraphs so they wrap to the window.
                let text = LICENSE
                    .split("\n\n")
                    .map(|p| p.lines().map(str::trim).collect::<Vec<_>>().join(" "))
                    .collect::<Vec<_>>()
                    .join("\n\n");
                egui::ScrollArea::vertical().max_height(250.0).show(ui, |ui| {
                    ui.label(RichText::new(text).size(13.0));
                });
                ui.checkbox(&mut self.accepted, "I accept the license terms");
            }
            Page::Options => {
                self.header(ui, "Install options", "Choose where and how to install.");
                ui.label("Install folder");
                ui.horizontal(|ui| {
                    let mut s = self.opts.dir.display().to_string();
                    if ui.add(egui::TextEdit::singleline(&mut s).desired_width(380.0)).changed() {
                        self.opts.dir = PathBuf::from(s);
                    }
                    if ui.button("Browse…").clicked()
                        && let Some(d) = rfd::FileDialog::new().set_directory(&self.opts.dir).pick_folder()
                    {
                        self.opts.dir = d.join(APP_NAME);
                    }
                });
                ui.add_space(8.0);
                ui.checkbox(&mut self.opts.start_menu_shortcut, "Start menu shortcut");
                ui.checkbox(&mut self.opts.desktop_shortcut, "Desktop shortcut");
                ui.checkbox(&mut self.opts.associate_files, "Open .soundesti project files with Soundesti");
                ui.checkbox(&mut self.launch_after, "Launch Soundesti when finished");
                let size: usize = entries().map(|e| e.iter().map(|x| x.data.len()).sum()).unwrap_or(0);
                ui.label(RichText::new(format!("Space required: {:.1} MB", size as f64 / 1_048_576.0)).color(DIM));
            }
            Page::Working => {
                let title = match self.action {
                    Action::Uninstall => "Uninstalling",
                    Action::Repair => "Repairing",
                    Action::Update => "Updating",
                    Action::Modify => "Applying changes",
                    Action::Install => "Installing",
                };
                self.header(ui, title, "Please wait…");
                if let Some(job) = &self.job {
                    let (f, m) = job.progress.lock().unwrap().clone();
                    ui.add(egui::ProgressBar::new(f).show_percentage());
                    ui.label(RichText::new(m).color(DIM));
                    if let Ok(res) = job.rx.try_recv() {
                        self.job = None;
                        match res {
                            Ok(msg) => {
                                self.message = msg;
                                self.page = Page::Done;
                            }
                            Err(e) => {
                                self.message = e;
                                self.page = Page::Error;
                            }
                        }
                    }
                }
                ctx.request_repaint();
            }
            Page::Done => {
                self.header(ui, "Finished", "");
                ui.label(&self.message);
                if self.action != Action::Uninstall {
                    ui.checkbox(&mut self.launch_after, "Launch Soundesti now");
                }
            }
            Page::Error => {
                self.header(ui, "Something went wrong", "Nothing was left half-installed: files are replaced one by one and the previous version keeps working.");
                ui.colored_label(WARN, &self.message);
            }
            Page::Maintenance => {
                let inst = self.installed.clone().unwrap();
                self.header(
                    ui,
                    &format!("{APP_NAME} is installed"),
                    &format!("Installed version {} · this installer {VERSION}", inst.version),
                );
                ui.label(RichText::new(inst.dir.display().to_string()).monospace().color(DIM));
                ui.add_space(10.0);
                let big = |ui: &mut egui::Ui, title: &str, desc: &str| -> bool {
                    let r = ui.add_sized([520.0, 44.0], egui::Button::new(RichText::new(title).strong()));
                    ui.label(RichText::new(desc).small().color(DIM));
                    ui.add_space(4.0);
                    r.clicked()
                };
                if big(ui, "🔧  Repair", "Check every file and restore anything missing or damaged.") {
                    match core::verify(&inst.dir) {
                        Ok(bad) if bad.is_empty() => {
                            self.online_status = "All files are intact. Repairing anyway (rewriting files)…".into()
                        }
                        Ok(bad) => self.online_status = format!("Repairing {} damaged file(s)…", bad.len()),
                        Err(_) => {}
                    }
                    self.start_job(Action::Repair);
                }
                if big(
                    ui,
                    "⬇  Force update / reinstall this version",
                    "Reinstall the files bundled in this installer, even if the same version is installed.",
                ) {
                    self.start_job(Action::Update);
                }
                if big(
                    ui,
                    "🌐  Check online for updates",
                    "Download the newest version from the Soundesti release server and install it.",
                ) {
                    self.start_online_update();
                    self.close_after_online = true;
                }
                if big(ui, "⚙  Modify", "Change shortcuts and file associations.") {
                    self.action = Action::Modify;
                    self.page = Page::Options;
                }
                if big(ui, "🗑  Uninstall", "Remove Soundesti from this computer.") {
                    self.page = Page::ConfirmUninstall;
                }
                if !self.online_status.is_empty() {
                    ui.colored_label(WARN, &self.online_status);
                }
                if let Some(rx) = &self.online
                    && let Ok(res) = rx.try_recv()
                {
                    self.online = None;
                    match res {
                        Ok(m) => {
                            self.online_status = m;
                            if self.close_after_online {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        Err(e) => self.online_status = e,
                    }
                }
                if self.online.is_some() {
                    ctx.request_repaint();
                }
            }
            Page::ConfirmUninstall => {
                self.header(
                    ui,
                    &format!("Uninstall {APP_NAME}?"),
                    "Your project files (.soundesti) are never deleted.",
                );
                ui.checkbox(&mut self.keep_data, "Keep my settings and autosaves");
            }
        }
    }

    fn buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let close = |ctx: &egui::Context| ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            match self.page {
                Page::Welcome => {
                    if ui.add_enabled(!PAYLOAD.is_empty(), egui::Button::new("Next >")).clicked() {
                        self.page = Page::License;
                    }
                    if ui.button("Cancel").clicked() {
                        close(ctx);
                    }
                }
                Page::License => {
                    if ui.add_enabled(self.accepted, egui::Button::new("Next >")).clicked() {
                        self.page = Page::Options;
                    }
                    if ui.button("< Back").clicked() {
                        self.page = Page::Welcome;
                    }
                }
                Page::Options => {
                    let modify = self.action == Action::Modify;
                    if ui.button(if modify { "Apply" } else { "Install" }).clicked() {
                        self.start_job(if modify { Action::Modify } else { Action::Install });
                    }
                    if ui.button("< Back").clicked() {
                        self.page = if modify { Page::Maintenance } else { Page::License };
                    }
                }
                Page::Working => {
                    ui.add_enabled(false, egui::Button::new("Please wait"));
                }
                Page::Done | Page::Error => {
                    if ui.button("Close").clicked() {
                        if self.page == Page::Done && self.launch_after && self.action != Action::Uninstall {
                            let _ = std::process::Command::new(self.opts.dir.join(APP_EXE))
                                .current_dir(&self.opts.dir)
                                .spawn();
                        }
                        close(ctx);
                    }
                }
                Page::Maintenance => {
                    if ui.button("Close").clicked() {
                        close(ctx);
                    }
                }
                Page::ConfirmUninstall => {
                    if ui.button(RichText::new("Uninstall").color(WARN)).clicked() {
                        self.start_job(Action::Uninstall);
                    }
                    if ui.button("< Back").clicked() {
                        self.page = if self.installed.is_some() { Page::Maintenance } else { Page::Welcome };
                    }
                }
            }
        });
    }
}

impl eframe::App for Wizard {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        egui::Panel::left("side")
            .exact_size(190.0)
            .frame(egui::Frame::new().fill(PANEL).inner_margin(14))
            .show(ui, |ui| self.sidebar(ui));
        egui::Panel::bottom("buttons")
            .frame(egui::Frame::new().inner_margin(12).fill(Color32::from_rgb(0x2a, 0x31, 0x38)))
            .show(ui, |ui| self.buttons(ui, &ctx));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(20).fill(Color32::from_rgb(0x2a, 0x31, 0x38)))
            .show(ui, |ui| self.content(ui, &ctx));
    }
}
