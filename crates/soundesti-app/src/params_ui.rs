//! Shared parameter grid: knobs, combo boxes and number fields grouped like a plugin panel.
//! Used by instrument windows and mixer effect windows.

use std::collections::BTreeMap;

use egui::{Color32, RichText, Ui};
use soundesti_plugin_api::{ParamInfo, ParamKind};

use crate::{theme, widgets};

#[derive(Default)]
pub struct GridEdit {
    /// A drag or edit started this frame (take an undo checkpoint).
    pub started: bool,
    /// The value that changed, by stable key.
    pub changed: Option<(&'static str, f32)>,
}

/// Draws `params` grouped by `group`, reading current values from `values`.
pub fn param_grid(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    params: &'static [ParamInfo],
    values: &BTreeMap<String, f32>,
    color: Color32,
) -> GridEdit {
    let mut edit = GridEdit::default();
    let mut groups: Vec<&str> = Vec::new();
    for p in params {
        if !groups.contains(&p.group) {
            groups.push(p.group);
        }
    }
    ui.horizontal_wrapped(|ui| {
        for g in groups {
            egui::Frame::new().fill(theme::PANEL_DARK).corner_radius(4).inner_margin(6).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new(g).strong().color(color));
                    ui.horizontal(|ui| {
                        for p in params.iter().filter(|p| p.group == g) {
                            let cur = values.get(p.key).copied().unwrap_or(p.default);
                            let mut v = cur;
                            let (started, changed) = ui.vertical(|ui| param_control(ui, salt, p, &mut v, color)).inner;
                            edit.started |= started;
                            if changed && v != cur {
                                edit.changed = Some((p.key, v));
                            }
                        }
                    });
                });
            });
        }
    });
    edit
}

fn param_control(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    p: &ParamInfo,
    v: &mut f32,
    color: Color32,
) -> (bool, bool) {
    ui.label(RichText::new(p.name).small());
    match p.kind {
        ParamKind::Stepped { labels } if !labels.is_empty() => {
            let mut idx = v.round() as usize;
            let mut ch = false;
            egui::ComboBox::from_id_salt((salt, p.id))
                .width(80.0)
                .selected_text(labels[idx.min(labels.len() - 1)])
                .show_ui(ui, |ui| {
                    for (i, l) in labels.iter().enumerate() {
                        ch |= ui.selectable_value(&mut idx, i, *l).changed();
                    }
                });
            *v = idx as f32;
            (ch, ch)
        }
        ParamKind::Stepped { .. } => {
            let r = ui.add(egui::DragValue::new(v).range(p.min..=p.max).speed(0.1).fixed_decimals(0).suffix(p.unit));
            (r.drag_started() || r.gained_focus(), r.changed())
        }
        ParamKind::Continuous => {
            // Frequencies use a log taper so the knob's middle is musically useful.
            let log = p.unit == "Hz" && p.min > 0.0;
            let to_norm = |x: f32| (x / p.min).ln() / (p.max / p.min).ln();
            let mut norm = if log { to_norm(*v) } else { *v };
            let (lo, hi, def) = if log { (0.0, 1.0, to_norm(p.default)) } else { (p.min, p.max, p.default) };
            let r = widgets::knob(
                ui,
                &mut norm,
                lo,
                hi,
                def,
                30.0,
                color,
                &format!("{} {}", p.name, fmt_value(*v, p.unit)),
            );
            if r.changed {
                *v = if log { p.min * (p.max / p.min).powf(norm) } else { norm };
            }
            ui.label(RichText::new(fmt_value(*v, p.unit)).small().color(theme::TEXT_DIM));
            (r.started, r.changed)
        }
    }
}

pub fn fmt_value(v: f32, unit: &str) -> String {
    match unit {
        "Hz" if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
        "Hz" => format!("{v:.0} Hz"),
        "s" if v < 1.0 => format!("{:.0} ms", v * 1000.0),
        "s" => format!("{v:.2} s"),
        "ct" => format!("{v:+.0} ct"),
        "Q" => format!("Q {v:.2}"),
        "dB" => format!("{v:+.1} dB"),
        "ms" => format!("{v:.0} ms"),
        ":1" => format!("{v:.1}:1"),
        "" => format!("{:.0}%", v * 100.0),
        u => format!("{v:.1} {u}"),
    }
}
