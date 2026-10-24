//! Vector icons drawn with egui shapes (no emoji or font glyphs), so they look identical
//! on every machine and stay sharp at any DPI. Shared with the installer via `#[path]`,
//! so this file depends only on egui.

#![allow(dead_code)]

use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Stop,
    Record,
    Pencil,
    Brush,
    Erase,
    Mute,
    Slice,
    Select,
    Zoom,
    Magnet,
    Keyboard,
    Ghost,
    Piano,
    Plus,
    Warning,
    Wrench,
    Download,
    Globe,
    Gear,
    Trash,
    Check,
    Metronome,
    Folder,
    Wave,
    Dot,
    ArrowRight,
    ArrowDown,
    Mixer,
    Rack,
    Song,
    Pattern,
}

/// Paints `icon` inside `rect` with `color`.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height());
    let o = rect.center() - vec2(s, s) * 0.5;
    let p = |x: f32, y: f32| -> Pos2 { pos2(o.x + x * s, o.y + y * s) };
    let w = (s * 0.11).max(1.2);
    let stroke = Stroke::new(w, color);
    let thin = Stroke::new((s * 0.075).max(1.0), color);
    let line = |pts: Vec<Pos2>, st: Stroke| painter.add(Shape::line(pts, st));
    let fill = |pts: Vec<Pos2>| painter.add(Shape::convex_polygon(pts, color, Stroke::NONE));
    match icon {
        Icon::Play => {
            fill(vec![p(0.3, 0.2), p(0.8, 0.5), p(0.3, 0.8)]);
        }
        Icon::Pause => {
            painter.rect_filled(Rect::from_min_max(p(0.27, 0.22), p(0.43, 0.78)), 1.0, color);
            painter.rect_filled(Rect::from_min_max(p(0.57, 0.22), p(0.73, 0.78)), 1.0, color);
        }
        Icon::Stop => {
            painter.rect_filled(Rect::from_min_max(p(0.25, 0.25), p(0.75, 0.75)), 1.5, color);
        }
        Icon::Record => {
            painter.circle_filled(p(0.5, 0.5), s * 0.26, color);
        }
        Icon::Pencil => {
            line(vec![p(0.3, 0.7), p(0.75, 0.25)], Stroke::new(s * 0.16, color));
            fill(vec![p(0.18, 0.82), p(0.24, 0.62), p(0.38, 0.76)]);
        }
        Icon::Brush => {
            line(vec![p(0.45, 0.55), p(0.8, 0.2)], stroke);
            painter.circle_filled(p(0.33, 0.67), s * 0.15, color);
            fill(vec![p(0.15, 0.85), p(0.22, 0.7), p(0.32, 0.8)]);
        }
        Icon::Erase => {
            line(vec![p(0.25, 0.25), p(0.75, 0.75)], stroke);
            line(vec![p(0.75, 0.25), p(0.25, 0.75)], stroke);
        }
        Icon::Mute => {
            fill(vec![p(0.15, 0.4), p(0.3, 0.4), p(0.48, 0.24), p(0.48, 0.76), p(0.3, 0.6), p(0.15, 0.6)]);
            line(vec![p(0.6, 0.38), p(0.84, 0.62)], thin);
            line(vec![p(0.84, 0.38), p(0.6, 0.62)], thin);
        }
        Icon::Slice => {
            // Knife: blade plus handle.
            fill(vec![p(0.62, 0.15), p(0.72, 0.15), p(0.72, 0.55), p(0.45, 0.85)]);
            painter.rect_filled(Rect::from_min_max(p(0.62, 0.55), p(0.74, 0.86)), 1.0, color);
            line(vec![p(0.25, 0.15), p(0.25, 0.85)], Stroke::new(w * 0.6, color.gamma_multiply(0.6)));
        }
        Icon::Select => {
            let r = Rect::from_min_max(p(0.2, 0.25), p(0.8, 0.75));
            let dash = |a: Pos2, b: Pos2| painter.add(Shape::dashed_line(&[a, b], thin, s * 0.09, s * 0.06));
            dash(r.left_top(), r.right_top());
            dash(r.right_top(), r.right_bottom());
            dash(r.right_bottom(), r.left_bottom());
            dash(r.left_bottom(), r.left_top());
        }
        Icon::Zoom => {
            painter.circle_stroke(p(0.43, 0.43), s * 0.22, stroke);
            line(vec![p(0.6, 0.6), p(0.82, 0.82)], Stroke::new(s * 0.14, color));
        }
        Icon::Magnet => {
            let pts: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let a = std::f32::consts::PI * i as f32 / 12.0;
                    p(0.5 - 0.25 * a.cos(), 0.55 + 0.25 * a.sin())
                })
                .collect();
            line(pts, Stroke::new(s * 0.15, color));
            line(vec![p(0.25, 0.55), p(0.25, 0.22)], Stroke::new(s * 0.15, color));
            line(vec![p(0.75, 0.55), p(0.75, 0.22)], Stroke::new(s * 0.15, color));
            painter.rect_filled(Rect::from_min_max(p(0.17, 0.15), p(0.33, 0.28)), 0.0, color.gamma_multiply(0.6));
            painter.rect_filled(Rect::from_min_max(p(0.67, 0.15), p(0.83, 0.28)), 0.0, color.gamma_multiply(0.6));
        }
        Icon::Keyboard => {
            painter.rect_stroke(Rect::from_min_max(p(0.1, 0.28), p(0.9, 0.72)), 2.0, thin, egui::StrokeKind::Inside);
            for r in 0..2 {
                for c in 0..5 {
                    let x = 0.2 + c as f32 * 0.13;
                    let y = 0.37 + r as f32 * 0.13;
                    painter.rect_filled(Rect::from_min_size(p(x, y), vec2(s * 0.08, s * 0.07)), 0.0, color);
                }
            }
        }
        Icon::Ghost => {
            painter.circle_filled(p(0.5, 0.42), s * 0.25, color);
            painter.rect_filled(Rect::from_min_max(p(0.25, 0.42), p(0.75, 0.75)), 0.0, color);
            for i in 0..3 {
                let x = 0.25 + i as f32 * 0.1667;
                fill(vec![p(x, 0.74), p(x + 0.1667, 0.74), p(x + 0.083, 0.86)]);
            }
            painter.circle_filled(p(0.41, 0.42), s * 0.05, Color32::BLACK);
            painter.circle_filled(p(0.59, 0.42), s * 0.05, Color32::BLACK);
        }
        Icon::Piano => {
            painter.rect_stroke(Rect::from_min_max(p(0.15, 0.2), p(0.85, 0.8)), 1.0, thin, egui::StrokeKind::Inside);
            for x in [0.38, 0.62] {
                line(vec![p(x, 0.2), p(x, 0.8)], thin);
            }
            for x in [0.32, 0.56, 0.74] {
                painter.rect_filled(Rect::from_min_max(p(x - 0.05, 0.2), p(x + 0.05, 0.55)), 0.0, color);
            }
        }
        Icon::Plus => {
            line(vec![p(0.5, 0.2), p(0.5, 0.8)], stroke);
            line(vec![p(0.2, 0.5), p(0.8, 0.5)], stroke);
        }
        Icon::Warning => {
            painter.add(Shape::closed_line(vec![p(0.5, 0.15), p(0.88, 0.82), p(0.12, 0.82)], thin));
            line(vec![p(0.5, 0.38), p(0.5, 0.6)], thin);
            painter.circle_filled(p(0.5, 0.7), s * 0.04, color);
        }
        Icon::Wrench => {
            line(vec![p(0.22, 0.78), p(0.58, 0.42)], Stroke::new(s * 0.14, color));
            painter.circle_stroke(p(0.67, 0.33), s * 0.15, stroke);
        }
        Icon::Download => {
            line(vec![p(0.5, 0.15), p(0.5, 0.62)], stroke);
            line(vec![p(0.3, 0.45), p(0.5, 0.65), p(0.7, 0.45)], stroke);
            line(vec![p(0.2, 0.82), p(0.8, 0.82)], stroke);
        }
        Icon::Globe => {
            painter.circle_stroke(p(0.5, 0.5), s * 0.33, thin);
            let ell: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let a = std::f32::consts::TAU * i as f32 / 24.0;
                    p(0.5 + 0.14 * a.cos(), 0.5 + 0.33 * a.sin())
                })
                .collect();
            line(ell, thin);
            line(vec![p(0.17, 0.5), p(0.83, 0.5)], thin);
        }
        Icon::Gear => {
            for i in 0..8 {
                let a = std::f32::consts::TAU * i as f32 / 8.0;
                line(
                    vec![p(0.5 + 0.22 * a.cos(), 0.5 + 0.22 * a.sin()), p(0.5 + 0.36 * a.cos(), 0.5 + 0.36 * a.sin())],
                    Stroke::new(s * 0.13, color),
                );
            }
            painter.circle_stroke(p(0.5, 0.5), s * 0.2, stroke);
        }
        Icon::Trash => {
            line(vec![p(0.2, 0.28), p(0.8, 0.28)], stroke);
            line(vec![p(0.4, 0.28), p(0.42, 0.18), p(0.58, 0.18), p(0.6, 0.28)], thin);
            painter.add(Shape::closed_line(vec![p(0.28, 0.32), p(0.72, 0.32), p(0.67, 0.84), p(0.33, 0.84)], thin));
        }
        Icon::Check => {
            line(vec![p(0.2, 0.52), p(0.42, 0.74), p(0.82, 0.28)], stroke);
        }
        Icon::Metronome => {
            painter.add(Shape::closed_line(vec![p(0.38, 0.15), p(0.62, 0.15), p(0.8, 0.85), p(0.2, 0.85)], thin));
            line(vec![p(0.5, 0.7), p(0.72, 0.25)], thin);
        }
        Icon::Folder => {
            fill(vec![p(0.12, 0.3), p(0.4, 0.3), p(0.46, 0.38), p(0.88, 0.38), p(0.88, 0.78), p(0.12, 0.78)]);
        }
        Icon::Wave => {
            let pts: Vec<Pos2> = (0..=24)
                .map(|i| {
                    let t = i as f32 / 24.0;
                    p(0.12 + 0.76 * t, 0.5 - 0.25 * (t * std::f32::consts::TAU * 1.5).sin())
                })
                .collect();
            line(pts, thin);
        }
        Icon::Dot => {
            painter.circle_filled(p(0.5, 0.5), s * 0.18, color);
        }
        Icon::ArrowRight => {
            fill(vec![p(0.35, 0.25), p(0.7, 0.5), p(0.35, 0.75)]);
        }
        Icon::Mixer => {
            for (x, k) in [(0.25, 0.62), (0.5, 0.35), (0.75, 0.55)] {
                line(vec![p(x, 0.15), p(x, 0.85)], thin);
                painter.rect_filled(Rect::from_center_size(p(x, k), vec2(0.16 * s, 0.1 * s)), 1.0, color);
            }
        }
        Icon::Rack => {
            for row in 0..3 {
                let y = 0.22 + row as f32 * 0.22;
                for col in 0..4 {
                    let x = 0.14 + col as f32 * 0.19;
                    let r = Rect::from_min_size(p(x, y), vec2(0.14 * s, 0.14 * s));
                    if (row + col) % 3 == 0 {
                        painter.rect_filled(r, 1.0, color);
                    } else {
                        painter.rect_stroke(r, 1.0, thin, egui::StrokeKind::Inside);
                    }
                }
            }
        }
        Icon::ArrowDown => {
            fill(vec![p(0.25, 0.35), p(0.75, 0.35), p(0.5, 0.7)]);
        }
        Icon::Song => {
            for (i, (x0, x1)) in [(0.12, 0.5), (0.4, 0.88), (0.2, 0.65)].iter().enumerate() {
                let y = 0.25 + i as f32 * 0.2;
                painter.rect_filled(Rect::from_min_max(p(*x0, y), p(*x1, y + 0.13)), 1.0, color);
            }
        }
        Icon::Pattern => {
            for r in 0..3 {
                for c in 0..4 {
                    let on = (r + c) % 2 == 0;
                    let rr =
                        Rect::from_min_size(p(0.14 + c as f32 * 0.19, 0.22 + r as f32 * 0.2), vec2(s * 0.15, s * 0.15));
                    painter.rect_filled(rr, 1.0, if on { color } else { color.gamma_multiply(0.35) });
                }
            }
        }
    }
}

/// Square icon-only button.
pub fn button(ui: &mut Ui, icon: Icon, selected: bool, tip: &str) -> Response {
    let size = ui.spacing().interact_size.y + 4.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    paint_button_frame(ui, rect, &resp, selected);
    let color = icon_color(ui, &resp, selected, true);
    paint(ui.painter(), rect.shrink(size * 0.16), icon, color);
    resp.on_hover_text(tip)
}

/// Icon followed by a text label, in one button.
pub fn text_button(ui: &mut Ui, icon: Icon, text: &str, selected: bool) -> Response {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, ui.visuals().text_color());
    let h = ui.spacing().interact_size.y + 4.0;
    let w = h + galley.size().x + 10.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(w, h), Sense::click());
    paint_button_frame(ui, rect, &resp, selected);
    let color = icon_color(ui, &resp, selected, true);
    let icon_rect = Rect::from_min_size(rect.min, vec2(h, h)).shrink(h * 0.2);
    paint(ui.painter(), icon_rect, icon, color);
    ui.painter().galley(pos2(rect.min.x + h, rect.center().y - galley.size().y / 2.0), galley, color);
    resp
}

/// A small inline icon (no interaction), e.g. in labels and status bars.
pub fn inline(ui: &mut Ui, icon: Icon, color: Color32) {
    let h = ui.spacing().interact_size.y * 0.8;
    let (rect, _) = ui.allocate_exact_size(vec2(h, h), Sense::hover());
    paint(ui.painter(), rect, icon, color);
}

fn paint_button_frame(ui: &Ui, rect: Rect, resp: &Response, selected: bool) {
    let v = ui.visuals();
    let fill = if selected {
        v.selection.bg_fill
    } else if resp.is_pointer_button_down_on() {
        v.widgets.active.bg_fill
    } else if resp.hovered() {
        v.widgets.hovered.bg_fill
    } else {
        v.widgets.inactive.weak_bg_fill
    };
    ui.painter().rect_filled(rect, 3.0, fill);
    if selected {
        ui.painter().rect_stroke(rect, 3.0, v.selection.stroke, egui::StrokeKind::Inside);
    }
}

fn icon_color(ui: &Ui, resp: &Response, selected: bool, enabled: bool) -> Color32 {
    let v = ui.visuals();
    if !enabled {
        v.weak_text_color()
    } else if selected || resp.hovered() {
        v.strong_text_color()
    } else {
        v.text_color()
    }
}
