//! Browser (Alt+F8): a folder tree of sample locations. Click a sample to preview it,
//! double-click to load it into a new Sampler channel, Enter to replace the selected channel's
//! sound (FL), Shift+Up/Down to step through samples with preview, or drag it onto a Channel
//! Rack row (replace) or the rack's empty area (new channel).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use egui::{RichText, Sense, Ui, vec2};

use crate::app::App;
use crate::icons::{self, Icon};
use crate::theme;

const MAX_SEARCH_RESULTS: usize = 500;
const MAX_SEARCH_DEPTH: usize = 6;

/// A file being dragged out of the Browser.
#[derive(Clone, Debug)]
pub struct DraggedSample(pub PathBuf);

#[derive(Clone)]
struct Entry {
    path: PathBuf,
    name: String,
    is_dir: bool,
}

#[derive(Default)]
pub struct BrowserView {
    /// Folders the user added with "Add folder" (saved in %APPDATA%\Soundesti\browser-folders.txt).
    user_roots: Vec<PathBuf>,
    loaded_roots: bool,
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, Vec<Entry>>,
    pub selected: Option<PathBuf>,
    filter: String,
    search: Option<(String, Vec<Entry>)>,
    pub auto_preview: bool,
    /// Visible files in display order, for arrow-key navigation.
    visible: Vec<PathBuf>,
}

impl BrowserView {
    pub fn new() -> Self {
        Self { auto_preview: true, ..Default::default() }
    }

    fn ensure_roots(&mut self) {
        if self.loaded_roots {
            return;
        }
        self.loaded_roots = true;
        if let Some(f) = folders_file()
            && let Ok(text) = std::fs::read_to_string(f)
        {
            self.user_roots = text.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect();
        }
        if let Some(packs) = packs_dir() {
            // Open the bundled pack on first use so there's something to click right away.
            self.expanded.insert(packs.clone());
            if let Some(first) = std::fs::read_dir(&packs).ok().and_then(|mut d| d.next()).and_then(Result::ok) {
                self.expanded.insert(first.path());
            }
        }
    }

    fn roots(&self) -> Vec<(String, PathBuf)> {
        let mut out = Vec::new();
        if let Some(p) = packs_dir() {
            out.push(("Packs".to_string(), p));
        }
        if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
            for name in ["Music", "Downloads", "Desktop", "Documents"] {
                let p = home.join(name);
                if p.is_dir() {
                    out.push((name.to_string(), p));
                }
            }
        }
        for p in &self.user_roots {
            let name =
                p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
            out.push((name, p.clone()));
        }
        out
    }

    fn save_roots(&self) {
        if let Some(f) = folders_file() {
            if let Some(dir) = f.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let text: Vec<String> = self.user_roots.iter().map(|p| p.display().to_string()).collect();
            let _ = std::fs::write(f, text.join("\n"));
        }
    }

    fn listing(&mut self, dir: &Path) -> Vec<Entry> {
        self.listings.entry(dir.to_path_buf()).or_insert_with(|| read_dir(dir)).clone()
    }

    pub fn refresh(&mut self) {
        self.listings.clear();
        self.search = None;
    }

    /// Moves the selection by `delta` visible files (arrow keys).
    pub fn step(&mut self, delta: i32) -> Option<PathBuf> {
        if self.visible.is_empty() {
            return None;
        }
        let cur = self.selected.as_ref().and_then(|s| self.visible.iter().position(|p| p == s));
        let next = match cur {
            Some(i) => (i as i32 + delta).clamp(0, self.visible.len() as i32 - 1) as usize,
            None => 0,
        };
        self.selected = Some(self.visible[next].clone());
        self.selected.clone()
    }
}

impl App {
    pub fn browser_command(&mut self, cmd: soundesti_commands::Command) -> bool {
        use soundesti_commands::Command::*;
        match cmd {
            BrowserReplaceChannel => {
                let (Some(path), Some(id)) = (self.browser.selected.clone(), self.selected_channel) else {
                    self.set_status("Select a sample in the Browser and a channel in the Channel Rack first.");
                    return true;
                };
                self.set_channel_sample(id, &path);
            }
            BrowserPreviewPrevious | BrowserPreviewNext => {
                let delta = if cmd == BrowserPreviewNext { 1 } else { -1 };
                if let Some(p) = self.browser.step(delta) {
                    self.preview_file(&p);
                }
            }
            _ => return false,
        }
        true
    }

    pub fn browser_ui(&mut self, ui: &mut Ui) {
        self.browser.ensure_roots();
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.browser.filter).hint_text("Search samples").desired_width(150.0),
            );
            if icons::button(ui, Icon::Plus, false, "Add a folder to the Browser").clicked()
                && let Some(dir) = rfd::FileDialog::new().set_title("Add folder to Browser").pick_folder()
                && !self.browser.user_roots.contains(&dir)
            {
                self.browser.user_roots.push(dir);
                self.browser.save_roots();
            }
            let auto = self.browser.auto_preview;
            if icons::button(ui, Icon::Wave, auto, "Preview samples when clicked").clicked() {
                self.browser.auto_preview = !auto;
            }
            if ui.small_button("Refresh").on_hover_text("Re-read folders from disk").clicked() {
                self.browser.refresh();
            }
        });
        ui.separator();
        self.browser.visible.clear();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let filter = self.browser.filter.trim().to_lowercase();
            if filter.is_empty() {
                for (name, path) in self.browser.roots() {
                    self.browser_dir_row(ui, &name, &path, 0, true);
                }
            } else {
                self.browser_search_results(ui, &filter);
            }
        });
        if let Some(p) = &self.browser.selected {
            ui.separator();
            ui.label(RichText::new(p.display().to_string()).small().color(theme::TEXT_DIM));
        }
    }

    fn browser_dir_row(&mut self, ui: &mut Ui, name: &str, path: &Path, depth: usize, is_root: bool) {
        let open = self.browser.expanded.contains(path);
        let r = ui
            .horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0);
                icons::inline(ui, if open { Icon::ArrowDown } else { Icon::ArrowRight }, theme::TEXT_DIM);
                icons::inline(ui, Icon::Folder, if is_root { theme::ACCENT } else { theme::TEXT_DIM });
                ui.add(egui::Label::new(RichText::new(name).color(theme::TEXT)).sense(Sense::click()))
            })
            .inner;
        if r.clicked() {
            if open {
                self.browser.expanded.remove(path);
            } else {
                self.browser.expanded.insert(path.to_path_buf());
            }
        }
        if is_root && self.browser.user_roots.iter().any(|p| p == path) {
            r.context_menu(|ui| {
                if ui.button("Remove from Browser").clicked() {
                    self.browser.user_roots.retain(|p| p != path);
                    self.browser.save_roots();
                    ui.close();
                }
            });
        }
        if !open {
            return;
        }
        let entries = self.browser.listing(path);
        if entries.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space((depth + 2) as f32 * 14.0);
                ui.label(RichText::new("(no WAV files)").small().color(theme::TEXT_DIM));
            });
        }
        for e in entries {
            if e.is_dir {
                self.browser_dir_row(ui, &e.name, &e.path, depth + 1, false);
            } else {
                self.browser_file_row(ui, &e, depth + 1);
            }
        }
    }

    fn browser_search_results(&mut self, ui: &mut Ui, filter: &str) {
        let stale = self.browser.search.as_ref().is_none_or(|(f, _)| f != filter);
        if stale {
            let mut found = Vec::new();
            for (_, root) in self.browser.roots() {
                search(&root, filter, 0, &mut found);
            }
            self.browser.search = Some((filter.to_string(), found));
        }
        let results = self.browser.search.as_ref().map(|(_, r)| r.clone()).unwrap_or_default();
        if results.is_empty() {
            ui.label(RichText::new("No matching samples.").color(theme::TEXT_DIM));
        }
        for e in &results {
            self.browser_file_row(ui, e, 0);
        }
        if results.len() >= MAX_SEARCH_RESULTS {
            ui.label(RichText::new("Showing the first 500 matches.").small().color(theme::TEXT_DIM));
        }
    }

    fn browser_file_row(&mut self, ui: &mut Ui, e: &Entry, depth: usize) {
        self.browser.visible.push(e.path.clone());
        let selected = self.browser.selected.as_ref() == Some(&e.path);
        let r = ui
            .horizontal(|ui| {
                ui.add_space(depth as f32 * 14.0 + 14.0);
                icons::inline(ui, Icon::Wave, if selected { theme::ACCENT } else { theme::TEXT_DIM });
                let text = RichText::new(&e.name).color(if selected { theme::ACCENT } else { theme::TEXT });
                ui.add(egui::Label::new(text).sense(Sense::click_and_drag()).selectable(false))
            })
            .inner
            .on_hover_text("Click: preview · Double-click: new channel · Enter: replace selected channel · Drag: onto the Channel Rack");
        r.dnd_set_drag_payload(DraggedSample(e.path.clone()));
        if r.dragged() {
            // A small ghost label follows the pointer while dragging.
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                egui::Area::new(egui::Id::unique("browser_drag_ghost"))
                    .fixed_pos(pos + vec2(12.0, 4.0))
                    .order(egui::Order::Tooltip)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(&e.name));
                    });
            }
        }
        if r.clicked() {
            self.browser.selected = Some(e.path.clone());
            if self.browser.auto_preview {
                self.preview_file(&e.path);
            }
        }
        if r.double_clicked() {
            self.add_sample_channel(&e.path);
        }
        r.context_menu(|ui| {
            if ui.button("Preview").clicked() {
                self.preview_file(&e.path);
                ui.close();
            }
            if ui.button("Open in new channel").clicked() {
                self.add_sample_channel(&e.path);
                ui.close();
            }
            let sel = self.selected_channel;
            if ui.add_enabled(sel.is_some(), egui::Button::new("Send to selected channel")).clicked()
                && let Some(id) = sel
            {
                self.set_channel_sample(id, &e.path);
                ui.close();
            }
        });
    }

    pub fn preview_file(&mut self, path: &Path) {
        if !self.session.preview_sample(&path.display().to_string()) {
            let err = self.session.sample_error.take().unwrap_or_else(|| "unknown error".into());
            self.set_status(format!("Could not preview {}: {err}", path.display()));
        }
    }

    /// New Sampler channel playing `path` (one undo step).
    pub fn add_sample_channel(&mut self, path: &Path) {
        if let Err(e) = soundesti_session::samples::load_wav(path) {
            self.set_status(format!("Could not load sample: {e}"));
            return;
        }
        self.checkpoint("Add sample channel");
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Sample".into());
        let id = self.project.add_channel(&stem, soundesti_synthesis::sampler::TYPE_ID);
        if let Some(c) = self.project.channel_mut(id) {
            c.instrument.sample = Some(path.display().to_string());
        }
        self.select_channel(id, false);
        self.needs_sync = true;
        self.set_status(format!("Added {stem} to the Channel Rack."));
    }
}

fn read_dir(dir: &Path) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<Entry> = rd
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let is_dir = e.file_type().ok()?.is_dir();
            (is_dir || soundesti_session::samples::is_supported(&path)).then_some(Entry { path, name, is_dir })
        })
        .collect();
    out.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    out
}

fn search(dir: &Path, filter: &str, depth: usize, out: &mut Vec<Entry>) {
    if depth > MAX_SEARCH_DEPTH || out.len() >= MAX_SEARCH_RESULTS {
        return;
    }
    for e in read_dir(dir) {
        if out.len() >= MAX_SEARCH_RESULTS {
            return;
        }
        if e.is_dir {
            search(&e.path, filter, depth + 1, out);
        } else if e.name.to_lowercase().contains(filter) {
            out.push(e);
        }
    }
}

fn folders_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Soundesti").join("browser-folders.txt"))
}

/// The bundled packs: `Packs\` next to the installed exe, or `assets\packs` in a source checkout.
pub fn packs_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let installed = dir.join("Packs");
    if installed.is_dir() {
        return Some(installed);
    }
    dir.ancestors().map(|a| a.join("assets").join("packs")).find(|p| p.is_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_puts_folders_first_and_hides_non_audio() {
        let root = std::env::temp_dir().join(format!("soundesti-browser-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("zeta")).unwrap();
        std::fs::write(root.join("b.wav"), b"").unwrap();
        std::fs::write(root.join("A.WAV"), b"").unwrap();
        std::fs::write(root.join("notes.txt"), b"").unwrap();
        let names: Vec<String> = read_dir(&root).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["zeta", "A.WAV", "b.wav"]);
        let mut found = Vec::new();
        search(&root, "b", 0, &mut found);
        assert_eq!(found.len(), 1);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
