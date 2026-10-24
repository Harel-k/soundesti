//! The Padz window: a 4x4 grid of pads (click to hear, right-click or drag a sample from
//! the Browser to load), plus volume / pan / tune for the selected pad.

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use soundesti_project::ChannelId;
use soundesti_synthesis::padz::{self, PADS};

use crate::app::App;
use crate::browser::DraggedSample;
use crate::theme;

/// Bundled samples a new Padz starts with (from the Soundesti Essentials pack).
const DEFAULT_KIT: [&str; PADS] = [
    "Kicks/SE Kick Punch",
    "Snares/SE Snare Crack",
    "Claps/SE Clap Classic",
    "Hats/SE Hat Closed",
    "Hats/SE Hat Open",
    "Kicks/SE Kick Deep",
    "Snares/SE Snare Rim",
    "Claps/SE Clap Snap",
    "Hats/SE Hat Closed Bright",
    "Hats/SE Ride Shimmer",
    "Kicks/SE Kick Boom 808",
    "Snares/SE Snare Fat",
    "FX/SE FX Impact",
    "FX/SE FX Crushed Zap",
    "Claps/SE Clap Wide Room",
    "FX/SE FX Noise Burst",
];

pub fn default_kit() -> Vec<Option<String>> {
    let Some(dir) = crate::browser::packs_dir().map(|d| d.join("Soundesti Essentials")) else { return Vec::new() };
    DEFAULT_KIT
        .iter()
        .map(|rel| Some(dir.join(format!("{rel}.wav"))).filter(|p| p.is_file()).map(|p| p.display().to_string()))
        .collect()
}

impl App {
    pub fn padz_ui(&mut self, ui: &mut Ui, id: ChannelId) {
        let Some(ch) = self.project.channel(id).cloned() else { return };
        let selected = *self.pad_selected.entry(id).or_insert(0);
        let color = theme::rgb(ch.color);
        let dragging = egui::DragAndDrop::has_payload_of_type::<DraggedSample>(ui.ctx());
        let mut audition: Option<usize> = None;
        let mut select: Option<usize> = None;
        let mut assign: Option<(usize, Option<std::path::PathBuf>)> = None;
        ui.label(RichText::new("Click a pad to hear it · right-click or drop a Browser sample to load · pad 1 = C5 (and C3 for GM drums)").small().color(theme::TEXT_DIM));
        // FL lays pads out bottom-left first; row 0 shown at the top holds pads 13-16.
        for row in (0..4).rev() {
            ui.horizontal(|ui| {
                for col in 0..4 {
                    let pad = row * 4 + col;
                    let name =
                        ch.instrument.pads.get(pad).cloned().flatten().and_then(|p| {
                            std::path::Path::new(&p).file_stem().map(|s| s.to_string_lossy().into_owned())
                        });
                    let (rect, resp) = ui.allocate_exact_size(vec2(96.0, 64.0), Sense::click());
                    let fill = if name.is_some() {
                        color.gamma_multiply(if pad == selected { 0.9 } else { 0.55 })
                    } else {
                        theme::PANEL_DARK
                    };
                    ui.painter().rect_filled(rect, 4.0, fill);
                    let stroke = if pad == selected {
                        Stroke::new(2.0, Color32::WHITE)
                    } else {
                        Stroke::new(1.0, theme::GRID_LINE)
                    };
                    ui.painter().rect_stroke(rect, 4.0, stroke, StrokeKind::Inside);
                    if dragging && resp.contains_pointer() {
                        ui.painter().rect_stroke(rect, 4.0, Stroke::new(2.0, theme::ACCENT), StrokeKind::Inside);
                    }
                    ui.painter().text(
                        rect.left_top() + vec2(6.0, 5.0),
                        egui::Align2::LEFT_TOP,
                        format!("{}", pad + 1),
                        egui::FontId::proportional(11.0),
                        theme::TEXT_DIM,
                    );
                    let label = name.unwrap_or_else(|| "(empty)".into());
                    let short: String = label.trim_start_matches("SE ").chars().take(14).collect();
                    ui.painter().text(
                        rect.center() + vec2(0.0, 6.0),
                        egui::Align2::CENTER_CENTER,
                        short,
                        egui::FontId::proportional(12.0),
                        if fill == theme::PANEL_DARK { theme::TEXT_DIM } else { Color32::BLACK },
                    );
                    if resp.clicked() {
                        audition = Some(pad);
                        select = Some(pad);
                    }
                    if let Some(p) = resp.dnd_release_payload::<DraggedSample>() {
                        assign = Some((pad, Some(p.0.clone())));
                    }
                    resp.context_menu(|ui| {
                        if ui.button("Load sample…").clicked() {
                            if let Some(p) = rfd::FileDialog::new().add_filter("WAV audio", &["wav"]).pick_file() {
                                assign = Some((pad, Some(p)));
                            }
                            ui.close();
                        }
                        if ui.button("Clear pad").clicked() {
                            assign = Some((pad, None));
                            ui.close();
                        }
                    });
                }
            });
        }
        if let Some(pad) = select {
            self.pad_selected.insert(id, pad);
        }
        if let Some(pad) = audition {
            self.preview_on(id, padz::FIRST_KEY + pad as u8, 0.9);
        } else if ui.input(|i| i.pointer.any_released()) && self.preview.is_some() {
            self.preview_off();
        }
        if let Some((pad, path)) = assign {
            self.set_pad_sample(id, pad, path);
        }
        // Knobs for the selected pad, then the master volume.
        let pad = *self.pad_selected.get(&id).unwrap_or(&0);
        ui.separator();
        let pad_params: &'static [soundesti_plugin_api::ParamInfo] = &padz::PARAMS[pad * 3..pad * 3 + 3];
        let master: &'static [soundesti_plugin_api::ParamInfo] = &padz::PARAMS[padz::MASTER as usize..];
        for params in [pad_params, master] {
            let edit =
                crate::params_ui::param_grid(ui, (id.0, pad, params.len()), params, &ch.instrument.params, color);
            if edit.started {
                self.checkpoint("Pad parameter");
            }
            if let Some((key, v)) = edit.changed
                && let Some(c) = self.project.channel_mut(id)
            {
                c.instrument.params.insert(key.to_string(), v);
                self.needs_sync = true;
            }
        }
    }

    pub fn set_pad_sample(&mut self, id: ChannelId, pad: usize, path: Option<std::path::PathBuf>) {
        if let Some(p) = &path
            && let Err(e) = soundesti_session::samples::load_wav(p)
        {
            self.set_status(format!("Could not load sample: {e}"));
            return;
        }
        self.checkpoint("Load pad sample");
        if let Some(c) = self.project.channel_mut(id) {
            if c.instrument.pads.len() < PADS {
                c.instrument.pads.resize(PADS, None);
            }
            c.instrument.pads[pad] = path.map(|p| p.display().to_string());
        }
        self.pad_selected.insert(id, pad);
        self.needs_sync = true;
    }
}
