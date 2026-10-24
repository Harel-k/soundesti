//! Custom DAW controls: knob, vertical fader, LED button, peak meter.
//!
//! Gesture conventions (FL-familiar): drag vertically to change, Ctrl-drag for fine
//! control, double-click to reset to default, mouse wheel nudges.

use egui::{Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use crate::theme;

/// Result of an interactive control: `started` fires once at the beginning of a
/// gesture so callers can record one undo step per drag.
pub struct ControlResponse {
    #[allow(dead_code)]
    pub response: Response,
    pub changed: bool,
    pub started: bool,
}

fn drag_value(
    ui: &Ui,
    resp: &Response,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    pixels: f32,
) -> (bool, bool) {
    let mut changed = false;
    let mut started = false;
    if resp.double_clicked() {
        started = true;
        *value = default;
        changed = true;
    } else if resp.drag_started() {
        started = true;
    }
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.ctrl);
        let dy = -resp.drag_delta().y;
        let range = max - min;
        let delta = dy / pixels * range * if fine { 0.1 } else { 1.0 };
        if delta != 0.0 {
            *value = (*value + delta).clamp(min, max);
            changed = true;
        }
    }
    if resp.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            started = true;
            *value = (*value + scroll.signum() * (max - min) / 100.0).clamp(min, max);
            changed = true;
        }
    }
    (changed, started)
}

#[allow(clippy::too_many_arguments)]
pub fn knob(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    size: f32,
    color: Color32,
    tip: &str,
) -> ControlResponse {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click_and_drag());
    let (changed, started) = drag_value(ui, &resp, value, min, max, default, 150.0);
    let p = ui.painter();
    let c = rect.center();
    let r = size * 0.42;
    p.circle_filled(c, r, theme::PANEL_DARK);
    p.circle_stroke(c, r, Stroke::new(1.0, Color32::from_gray(90)));
    // 270° sweep from -135° to +135°, 0 at top.
    let t = if max > min { (*value - min) / (max - min) } else { 0.0 };
    let bipolar = min < 0.0 && max > 0.0;
    let angle_of = |t: f32| (-135.0 + 270.0 * t).to_radians();
    let arc = |from: f32, to: f32| -> Vec<Pos2> {
        let n = 24;
        (0..=n)
            .map(|i| {
                let a = angle_of(from + (to - from) * i as f32 / n as f32);
                c + vec2(a.sin(), -a.cos()) * (r - 2.0)
            })
            .collect()
    };
    let zero_t = if bipolar { (0.0 - min) / (max - min) } else { 0.0 };
    let (a, b) = if t < zero_t { (t, zero_t) } else { (zero_t, t) };
    if b > a {
        p.add(egui::Shape::line(arc(a, b), Stroke::new(2.5, color)));
    }
    let ang = angle_of(t);
    p.line_segment([c, c + vec2(ang.sin(), -ang.cos()) * (r - 1.0)], Stroke::new(2.0, theme::TEXT));
    let resp = resp.on_hover_text(tip);
    ControlResponse { response: resp, changed, started }
}

pub fn fader(
    ui: &mut Ui,
    value: &mut f32,
    max: f32,
    default: f32,
    size: Vec2,
    color: Color32,
    tip: &str,
) -> ControlResponse {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let (changed, started) = drag_value(ui, &resp, value, 0.0, max, default, size.y);
    let p = ui.painter();
    let track = Rect::from_center_size(rect.center(), vec2(4.0, rect.height() - 8.0));
    p.rect_filled(track, 2.0, theme::PANEL_DARK);
    let t = (*value / max).clamp(0.0, 1.0);
    let y = track.bottom() - t * track.height();
    p.rect_filled(Rect::from_min_max(pos2(track.left(), y), track.right_bottom()), 2.0, color.gamma_multiply(0.6));
    let cap = Rect::from_center_size(pos2(rect.center().x, y), vec2(size.x - 2.0, 10.0));
    p.rect_filled(cap, 2.0, Color32::from_gray(200));
    p.hline(cap.x_range(), y, Stroke::new(1.0, Color32::BLACK));
    let resp = resp.on_hover_text(tip);
    ControlResponse { response: resp, changed, started }
}

/// Small round toggle (mute LEDs). Returns the click response.
pub fn led(ui: &mut Ui, on: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::click());
    let col = if on { theme::LED_ON } else { theme::LED_OFF };
    ui.painter().circle_filled(rect.center(), 4.5, col);
    if resp.hovered() {
        ui.painter().circle_stroke(rect.center(), 6.0, Stroke::new(1.0, theme::TEXT_DIM));
    }
    resp.on_hover_text(tip)
}

/// Peak meter with simple decay; `level` values are linear peak (0..1+).
pub fn meter(ui: &mut Ui, rect: Rect, left: f32, right: f32) {
    let p = ui.painter();
    p.rect_filled(rect, 1.0, Color32::from_gray(18));
    let w = (rect.width() - 1.0) / 2.0;
    for (i, lvl) in [left, right].into_iter().enumerate() {
        let x0 = rect.left() + i as f32 * (w + 1.0);
        // Map -60..+6 dB to 0..1.
        let db = if lvl > 0.0 { 20.0 * lvl.log10() } else { -100.0 };
        let t = ((db + 60.0) / 66.0).clamp(0.0, 1.0);
        let top = rect.bottom() - t * rect.height();
        let col = if db > 0.0 {
            theme::METER_HIGH
        } else if db > -6.0 {
            theme::METER_MID
        } else {
            theme::METER_LOW
        };
        p.rect_filled(Rect::from_min_max(pos2(x0, top), pos2(x0 + w, rect.bottom())), 0.0, col);
    }
    // 0 dB tick.
    let zero = rect.bottom() - (60.0 / 66.0) * rect.height();
    p.hline(rect.x_range(), zero, Stroke::new(1.0, Color32::from_gray(120)));
    p.rect_stroke(rect, 1.0, Stroke::new(1.0, Color32::from_gray(60)), StrokeKind::Inside);
}

/// Peak-hold/decay state for one stereo meter.
#[derive(Default, Clone, Copy)]
pub struct MeterState {
    pub left: f32,
    pub right: f32,
}

impl MeterState {
    /// Feed new peaks; decays ~20 dB/s at 60 fps.
    pub fn update(&mut self, (l, r): (f32, f32), dt: f32) {
        let decay = 10f32.powf(-20.0 * dt / 20.0);
        self.left = (self.left * decay).max(l);
        self.right = (self.right * decay).max(r);
    }
}
