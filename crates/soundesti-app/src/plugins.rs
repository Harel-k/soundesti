//! Third-party plugins in the app: the plugin manager (scan, folders, problems), plugin
//! entries in the ADD / Channel Rack / Mixer menus, plugin editor windows and a generic
//! parameter list for every plugin.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use egui::{RichText, Ui};
use soundesti_plugin_host::editor::{self, EditorWindow};
use soundesti_plugin_host::scan::{self, PluginCache};
use soundesti_plugin_host::{PluginDesc, PluginFormat, PluginInstance};
use soundesti_project::PLUGIN_TYPE_ID;
use soundesti_session::PluginKey;

use crate::app::App;
use crate::{icons, theme};

struct ScanJob {
    progress: Arc<Mutex<(usize, usize, String)>>,
    cancel: Arc<AtomicBool>,
    done: Receiver<PluginCache>,
}

struct OpenEditor {
    key: PluginKey,
    plugin: Arc<dyn PluginInstance>,
    window: EditorWindow,
}

pub struct PluginUi {
    pub cache: PluginCache,
    scan: Option<ScanJob>,
    editors: Vec<OpenEditor>,
    pub main_hwnd: isize,
    filter: String,
    /// Parameter-list search per plugin window.
    param_filter: String,
}

impl PluginUi {
    pub fn new(main_hwnd: isize) -> Self {
        Self {
            cache: PluginCache::load(),
            scan: None,
            editors: Vec::new(),
            main_hwnd,
            filter: String::new(),
            param_filter: String::new(),
        }
    }

    pub fn scanning(&self) -> bool {
        self.scan.is_some()
    }

    pub fn any_editor_open(&self) -> bool {
        !self.editors.is_empty()
    }
}

/// Native handle of the main window (owner of plugin editor windows).
pub fn main_window_handle(cc: &eframe::CreationContext<'_>) -> isize {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match cc.window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

impl App {
    /// Starts a background scan of every plugin folder (results replace the cache).
    pub fn start_plugin_scan(&mut self) {
        if self.plugins.scan.is_some() {
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let progress = Arc::new(Mutex::new((0, 0, String::new())));
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let mut cache = self.plugins.cache.clone();
        let (p2, c2) = (progress.clone(), cancel.clone());
        std::thread::spawn(move || {
            scan::rescan(&mut cache, &exe, |done, total, file| {
                if let Ok(mut p) = p2.lock() {
                    *p = (done, total, file.display().to_string());
                }
                !c2.load(Ordering::Relaxed)
            });
            let _ = tx.send(cache);
        });
        self.plugins.scan = Some(ScanJob { progress, cancel, done: rx });
    }

    /// Per-frame plugin housekeeping: scan results, editor idle, closed editor windows.
    pub fn tick_plugins(&mut self) {
        if let Some(job) = &self.plugins.scan
            && let Ok(cache) = job.done.try_recv()
        {
            let found = cache.plugins().len();
            cache.save();
            self.plugins.cache = cache;
            self.plugins.scan = None;
            self.set_status(format!("Plugin scan finished: {found} plugins found."));
        }
        for hwnd in editor::take_close_requests() {
            if let Some(i) = self.plugins.editors.iter().position(|e| e.window.handle() == hwnd) {
                let e = self.plugins.editors.remove(i);
                e.plugin.close_editor();
            }
        }
        // Editors whose plugin was removed or replaced close before the plugin is freed.
        let mut i = 0;
        while i < self.plugins.editors.len() {
            let live = self.session.plugin(self.plugins.editors[i].key);
            if live.is_some_and(|p| Arc::ptr_eq(&p, &self.plugins.editors[i].plugin)) {
                self.plugins.editors[i].plugin.editor_idle();
                i += 1;
            } else {
                let e = self.plugins.editors.remove(i);
                e.plugin.close_editor();
            }
        }
        for e in std::mem::take(&mut self.session.plugin_errors) {
            self.set_status(format!("Plugin problem: {e}"));
        }
    }

    pub fn open_plugin_editor(&mut self, key: PluginKey) {
        if let Some(e) = self.plugins.editors.iter().find(|e| e.key == key) {
            e.window.focus();
            return;
        }
        let Some(plugin) = self.session.plugin(key) else {
            self.set_status("This plugin is not running (it failed to load).");
            return;
        };
        if !plugin.has_editor() {
            self.set_status(format!("{} has no window of its own; use the knobs in Soundesti.", plugin.desc().name));
            return;
        }
        let title = match key {
            PluginKey::Channel(id) => self.project.channel(id).map(|c| c.name.clone()).unwrap_or_default(),
            PluginKey::Effect(t, s) => format!(
                "{} - {} slot {}",
                plugin.desc().name,
                self.project.mixer.get(t).map(|m| m.name.as_str()).unwrap_or(""),
                s + 1
            ),
        };
        let Some(window) = EditorWindow::create(self.plugins.main_hwnd, &title) else {
            self.set_status("Could not create the plugin window.");
            return;
        };
        match plugin.open_editor(window.handle()) {
            Some((w, h)) => {
                window.show_with_client_size(w, h);
                self.plugins.editors.push(OpenEditor { key, plugin, window });
            }
            None => self.set_status(format!("{} did not open its window.", plugin.desc().name)),
        }
    }

    /// Adds a channel playing an instrument plugin and opens its window (as FL does).
    pub fn add_plugin_channel(&mut self, d: &PluginDesc) {
        self.checkpoint("Add plugin channel");
        let id = self.project.add_channel(&d.name, PLUGIN_TYPE_ID);
        if let Some(c) = self.project.channel_mut(id) {
            c.instrument.plugin = Some(soundesti_session::plugin_ref(d));
        }
        self.select_channel(id, false);
        self.needs_sync = true;
        self.pending_editor = Some(PluginKey::Channel(id));
        self.set_status(format!("Added {} ({}).", d.name, d.format.label()));
    }

    pub fn set_plugin_effect(&mut self, track: usize, slot: usize, d: &PluginDesc) {
        self.checkpoint("Add effect plugin");
        self.project.mixer[track].set_plugin_effect(slot, soundesti_session::plugin_ref(d));
        self.needs_sync = true;
        self.pending_editor = Some(PluginKey::Effect(track, slot));
    }

    /// Plugin section for the ADD menu / Channel Rack "+" menu.
    pub fn plugin_instrument_menu(&mut self, ui: &mut Ui) {
        let list: Vec<PluginDesc> = self.plugins.cache.plugins().into_iter().filter(|p| p.is_instrument).collect();
        ui.separator();
        ui.label(RichText::new("Plugins").small().color(theme::TEXT_DIM));
        if list.is_empty() {
            ui.label(RichText::new("None found yet").small().color(theme::TEXT_DIM));
        }
        for d in list {
            if ui
                .button(format!("{}  ({})", d.name, d.format.label()))
                .on_hover_text(format!("{} · {}", d.vendor, d.path.display()))
                .clicked()
            {
                self.add_plugin_channel(&d);
                ui.close();
            }
        }
        if ui.button("Manage plugins…").clicked() {
            self.win.plugin_manager = true;
            ui.close();
        }
    }

    /// Plugin section for a mixer slot's effect menu.
    pub fn plugin_effect_menu(&mut self, ui: &mut Ui, track: usize, slot: usize) {
        let list: Vec<PluginDesc> = self.plugins.cache.plugins().into_iter().filter(|p| !p.is_instrument).collect();
        ui.separator();
        ui.label(RichText::new("Plugins").small().color(theme::TEXT_DIM));
        if list.is_empty() {
            ui.label(RichText::new("None found yet").small().color(theme::TEXT_DIM));
        }
        for d in list {
            if ui
                .button(format!("{}  ({})", d.name, d.format.label()))
                .on_hover_text(format!("{} · {}", d.vendor, d.path.display()))
                .clicked()
            {
                self.set_plugin_effect(track, slot, &d);
                ui.close();
            }
        }
    }

    /// Body of a plugin's Soundesti window: open its own window, plus every parameter.
    pub fn plugin_params_ui(&mut self, ui: &mut Ui, key: PluginKey) {
        let Some(plugin) = self.session.plugin(key) else {
            ui.label(RichText::new("This plugin could not be loaded. It may have been moved or uninstalled; check OPTIONS > Manage plugins.").color(theme::WARN));
            return;
        };
        let d = plugin.desc().clone();
        ui.horizontal(|ui| {
            ui.label(RichText::new(&d.name).strong());
            ui.label(RichText::new(format!("{} · {}", d.vendor, d.format.label())).small().color(theme::TEXT_DIM));
        });
        ui.horizontal(|ui| {
            if plugin.has_editor() && icons::text_button(ui, icons::Icon::Wrench, "Open plugin window", false).clicked()
            {
                self.open_plugin_editor(key);
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.plugins.param_filter)
                    .hint_text("Search parameters")
                    .desired_width(140.0),
            );
        });
        let n = plugin.param_count();
        if n == 0 {
            return;
        }
        let filter = self.plugins.param_filter.to_lowercase();
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            egui::Grid::new(("plugin_params", format!("{key:?}"))).num_columns(3).striped(true).show(ui, |ui| {
                for i in 0..n.min(512) {
                    let name = plugin.param_name(i);
                    if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                        continue;
                    }
                    ui.label(if name.is_empty() { format!("Param {}", i + 1) } else { name });
                    let mut v = plugin.param_value(i);
                    if ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false)).changed() {
                        plugin.set_param_value(i, v);
                    }
                    ui.label(RichText::new(plugin.param_display(i)).small().color(theme::TEXT_DIM));
                    ui.end_row();
                }
            });
        });
        if n > 512 {
            ui.label(
                RichText::new(format!("Showing the first 512 of {n} parameters; use the plugin window for the rest."))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
    }

    pub fn plugin_manager_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| match &self.plugins.scan {
            Some(job) => {
                let (done, total, file) = job.progress.lock().map(|p| p.clone()).unwrap_or_default();
                ui.add(
                    egui::ProgressBar::new(if total == 0 { 0.0 } else { done as f32 / total as f32 })
                        .desired_width(180.0)
                        .text(format!("{done}/{total}")),
                );
                if ui.button("Stop").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                ui.label(RichText::new(file).small().color(theme::TEXT_DIM));
            }
            None => {
                if icons::text_button(ui, icons::Icon::Zoom, "Find plugins", false)
                    .on_hover_text("Scan every plugin folder (new and changed files only)")
                    .clicked()
                {
                    self.start_plugin_scan();
                }
                if ui.button("Add folder…").clicked()
                    && let Some(dir) = rfd::FileDialog::new().set_title("Add a plugin folder").pick_folder()
                {
                    for f in [PluginFormat::Vst3, PluginFormat::Clap, PluginFormat::Vst2] {
                        if !self.plugins.cache.user_dirs.iter().any(|(ff, d)| *ff == f && *d == dir) {
                            self.plugins.cache.user_dirs.push((f, dir.clone()));
                        }
                    }
                    self.plugins.cache.save();
                    self.start_plugin_scan();
                }
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.plugins.filter).hint_text("Search plugins").desired_width(220.0));
        let filter = self.plugins.filter.to_lowercase();
        let list = self.plugins.cache.plugins();
        egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
            for (title, inst) in [("Instruments", true), ("Effects", false)] {
                ui.label(RichText::new(title).strong().color(theme::ACCENT));
                let mut any = false;
                for d in list.iter().filter(|d| {
                    d.is_instrument == inst
                        && (filter.is_empty()
                            || d.name.to_lowercase().contains(&filter)
                            || d.vendor.to_lowercase().contains(&filter))
                }) {
                    any = true;
                    ui.horizontal(|ui| {
                        if inst && ui.small_button("Add").on_hover_text("Add to the Channel Rack").clicked() {
                            self.add_plugin_channel(d);
                        }
                        ui.label(&d.name);
                        ui.label(
                            RichText::new(format!("{} · {}", d.vendor, d.format.label()))
                                .small()
                                .color(theme::TEXT_DIM),
                        )
                        .on_hover_text(d.path.display().to_string());
                    });
                }
                if !any {
                    ui.label(RichText::new("None").small().color(theme::TEXT_DIM));
                }
            }
            let failures: Vec<(PathBuf, String)> =
                self.plugins.cache.failures().map(|e| (e.path.clone(), e.error.clone().unwrap_or_default())).collect();
            if !failures.is_empty() {
                ui.collapsing(format!("{} file(s) could not be loaded", failures.len()), |ui| {
                    for (p, e) in failures {
                        ui.label(RichText::new(format!("{} — {e}", p.display())).small().color(theme::TEXT_DIM));
                    }
                });
            }
        });
        ui.separator();
        ui.label(RichText::new("Folders searched:").small());
        for (f, d) in self.plugins.cache.all_dirs() {
            if d.is_dir() {
                ui.label(RichText::new(format!("{}  {}", f.label(), d.display())).small().color(theme::TEXT_DIM));
            }
        }
        ui.label(
            RichText::new("Plugins run inside Soundesti: a plugin that crashes closes Soundesti too, so save often. FL Studio's own plugins are available through \"FL Studio VSTi\" if FL Studio is installed.")
                .small()
                .color(theme::TEXT_DIM),
        );
    }
}
