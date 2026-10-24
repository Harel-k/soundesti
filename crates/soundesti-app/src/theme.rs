//! Soundesti's original dark theme. Dense, high-contrast, FL-familiar in layout only.

use egui::{Color32, CornerRadius, Stroke, Visuals};

pub const BG: Color32 = Color32::from_rgb(0x1d, 0x22, 0x27);
pub const PANEL: Color32 = Color32::from_rgb(0x2a, 0x31, 0x38);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x23, 0x29, 0x2f);
pub const GRID_BG: Color32 = Color32::from_rgb(0x30, 0x38, 0x40);
pub const GRID_BLACK_KEY: Color32 = Color32::from_rgb(0x28, 0x2f, 0x36);
pub const GRID_LINE: Color32 = Color32::from_rgb(0x3b, 0x44, 0x4d);
pub const GRID_BEAT: Color32 = Color32::from_rgb(0x4a, 0x55, 0x60);
pub const GRID_BAR: Color32 = Color32::from_rgb(0x66, 0x73, 0x80);
pub const TEXT: Color32 = Color32::from_rgb(0xd8, 0xde, 0xe4);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8a, 0x96, 0xa2);
/// Soundesti accent: teal.
pub const ACCENT: Color32 = Color32::from_rgb(0x2e, 0xd3, 0xb7);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x1e, 0x7f, 0x70);
pub const WARN: Color32 = Color32::from_rgb(0xff, 0xa0, 0x40);
pub const DANGER: Color32 = Color32::from_rgb(0xff, 0x55, 0x55);
pub const LED_ON: Color32 = Color32::from_rgb(0x6c, 0xf0, 0x7a);
pub const LED_OFF: Color32 = Color32::from_rgb(0x3a, 0x44, 0x3c);
pub const STEP_ON: Color32 = Color32::from_rgb(0xe8, 0xec, 0xf0);
pub const STEP_A: Color32 = Color32::from_rgb(0x4a, 0x52, 0x5c);
pub const STEP_B: Color32 = Color32::from_rgb(0x5e, 0x4a, 0x50);
pub const PLAYHEAD: Color32 = Color32::from_rgb(0x9f, 0xff, 0x8a);
pub const METER_LOW: Color32 = Color32::from_rgb(0x3d, 0xdc, 0x84);
pub const METER_MID: Color32 = Color32::from_rgb(0xf5, 0xd4, 0x42);
pub const METER_HIGH: Color32 = Color32::from_rgb(0xff, 0x4d, 0x4d);

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = PANEL_DARK;
    v.faint_bg_color = PANEL_DARK;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT_DIM;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.window_corner_radius = CornerRadius::same(4);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x44, 0x4e, 0x58));
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.inactive.bg_fill = Color32::from_rgb(0x3a, 0x43, 0x4c);
    v.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x3a, 0x43, 0x4c);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x46, 0x51, 0x5c);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x46, 0x51, 0x5c);
    v.widgets.active.bg_fill = ACCENT_DIM;
    ctx.set_visuals(v);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 4.0);
        s.spacing.button_padding = egui::vec2(6.0, 2.0);
        s.spacing.interact_size.y = 20.0;
    });
}
