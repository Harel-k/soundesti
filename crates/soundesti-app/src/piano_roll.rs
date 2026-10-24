//! Piano roll: note grid, keyboard, velocity lane, tools and editing commands.
//!
//! Pointer handling is an explicit gesture state machine ([`Drag`]): a gesture starts
//! on button press, updates while held, and commits one undo step on release. Escape
//! (view.close_window) cancels the active gesture and restores the pre-gesture state.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use soundesti_audio::Command as EngineCmd;
use soundesti_commands::Command;
use soundesti_project::{ChannelId, Note, Project};
use soundesti_timeline::{PPQ, TICKS_PER_STEP, Tick, snap_floor, snap_round};

use crate::app::{App, Tool};
use crate::theme;

const KEYS_W: f32 = 56.0;
const RULER_H: f32 = 20.0;
const LANE_H: f32 = 72.0;
const EDGE_PX: f32 = 6.0;

pub struct PianoRoll {
    pub tool: Tool,
    pub px_per_tick: f32,
    pub row_h: f32,
    pub scroll_tick: f32,
    pub top_key: f32,
    pub last_len: Tick,
    pub ghost: bool,
    pub clipboard: Vec<Note>,
    last_grid_w: f32,
    drag: Option<Drag>,
}

impl Default for PianoRoll {
    fn default() -> Self {
        Self {
            tool: Tool::Draw,
            px_per_tick: 0.12,
            row_h: 14.0,
            scroll_tick: 0.0,
            top_key: 84.0,
            last_len: TICKS_PER_STEP,
            ghost: false,
            clipboard: Vec::new(),
            last_grid_w: 800.0,
            drag: None,
        }
    }
}

struct Drag {
    kind: DragKind,
    before: Project,
    changed: bool,
    label: &'static str,
}

enum DragKind {
    Move { anchor_tick: f32, anchor_key: i32, originals: Vec<(usize, Note)>, lead: usize },
    Resize { anchor_tick: f32, originals: Vec<(usize, Note)>, lead: usize },
    Select { start: Pos2, additive: bool },
    Erase,
    Paint { next_tick: Tick },
    Velocity,
    Keyboard,
    Pan,
}

impl PianoRoll {
    /// Escape during a gesture: restore the state from before it. Returns true if a
    /// gesture was active.
    pub fn cancel_drag(&mut self, project: &mut Project) -> bool {
        match self.drag.take() {
            Some(d) => {
                if d.changed {
                    *project = d.before;
                }
                true
            }
            None => false,
        }
    }
}

#[derive(Clone, Copy)]
struct View {
    grid: Rect,
    px: f32,
    scroll: f32,
    top_key: f32,
    row_h: f32,
}

impl View {
    fn x(&self, tick: f32) -> f32 {
        self.grid.left() + (tick - self.scroll) * self.px
    }
    fn tick(&self, x: f32) -> f32 {
        self.scroll + (x - self.grid.left()) / self.px
    }
    fn y(&self, key: i32) -> f32 {
        self.grid.top() + (self.top_key - key as f32) * self.row_h
    }
    fn key(&self, y: f32) -> i32 {
        (self.top_key - (y - self.grid.top()) / self.row_h).ceil() as i32
    }
    fn note_rect(&self, n: &Note) -> Rect {
        let y = self.y(n.key as i32);
        Rect::from_min_max(
            pos2(self.x(n.start as f32), y + 1.0),
            pos2(self.x(n.end() as f32).max(self.x(n.start as f32) + 3.0), y + self.row_h - 1.0),
        )
    }
}

const NOTE_NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];

/// FL naming: MIDI 60 = C5.
pub fn note_name(key: i32) -> String {
    format!("{}{}", NOTE_NAMES[key.rem_euclid(12) as usize], key.div_euclid(12))
}

fn is_black(key: i32) -> bool {
    matches!(key.rem_euclid(12), 1 | 3 | 6 | 8 | 10)
}

impl App {
    fn snap_or(&self, fallback: Tick) -> Tick {
        if self.snap > 0 { self.snap } else { fallback }
    }

    fn pr_notes(&mut self, ch: ChannelId) -> &mut Vec<Note> {
        self.project.current_mut().notes_mut(ch)
    }

    pub fn piano_roll_ui(&mut self, ui: &mut Ui) {
        let Some(ch) = self.selected_channel else {
            ui.label("Select a channel in the Channel Rack to edit its notes.");
            return;
        };
        ui.horizontal(|ui| {
            for t in Tool::ALL {
                if crate::icons::button(ui, t.icon(), self.piano.tool == t, t.label()).clicked() {
                    self.piano.tool = t;
                }
            }
            ui.separator();
            if crate::icons::text_button(ui, crate::icons::Icon::Ghost, "Ghosts", self.piano.ghost)
                .on_hover_text("Ghost channels (Alt+V)")
                .clicked()
            {
                self.piano.ghost = !self.piano.ghost;
            }
            if ui.button("Quantize…").on_hover_text("Alt+Q").clicked() {
                self.win.quantize = true;
            }
            ui.separator();
            let name = self.project.channel(ch).map(|c| c.name.clone()).unwrap_or_default();
            egui::ComboBox::from_id_salt("pr_channel").selected_text(name).show_ui(ui, |ui| {
                let list: Vec<_> = self.project.channels.iter().map(|c| (c.id, c.name.clone())).collect();
                for (id, n) in list {
                    if ui.selectable_label(id == ch, n).clicked() {
                        self.select_channel(id, false);
                    }
                }
            });
            let count = self.project.current().notes(ch).len();
            let sel = self.project.current().notes(ch).iter().filter(|n| n.selected).count();
            ui.label(egui::RichText::new(format!("{count} notes, {sel} selected")).color(theme::TEXT_DIM).small());
        });
        self.piano_canvas(ui, ch);
    }

    fn piano_canvas(&mut self, ui: &mut Ui, ch: ChannelId) {
        let size = ui.available_size().max(vec2(300.0, 200.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let keys_rect = Rect::from_min_max(
            pos2(rect.left(), rect.top() + RULER_H),
            pos2(rect.left() + KEYS_W, rect.bottom() - LANE_H),
        );
        let ruler =
            Rect::from_min_max(pos2(rect.left() + KEYS_W, rect.top()), pos2(rect.right(), rect.top() + RULER_H));
        let grid = Rect::from_min_max(
            pos2(rect.left() + KEYS_W, rect.top() + RULER_H),
            pos2(rect.right(), rect.bottom() - LANE_H),
        );
        let lane = Rect::from_min_max(pos2(rect.left() + KEYS_W, rect.bottom() - LANE_H + 4.0), rect.right_bottom());
        self.piano.last_grid_w = grid.width();

        self.piano_input(ui, &resp, ch, rect, keys_rect, ruler, grid, lane);

        let v = View {
            grid,
            px: self.piano.px_per_tick,
            scroll: self.piano.scroll_tick,
            top_key: self.piano.top_key,
            row_h: self.piano.row_h,
        };
        let painter = ui.painter_at(rect);
        self.draw_grid(&painter, v, ch);
        self.draw_keyboard(&painter, v, keys_rect);
        self.draw_ruler(&painter, v, ruler);
        self.draw_velocity_lane(&painter, v, lane, ch);
        if let Some(Drag { kind: DragKind::Select { start, .. }, .. }) = &self.piano.drag
            && let Some(p) = ui.input(|i| i.pointer.interact_pos())
        {
            let r = Rect::from_two_pos(*start, p);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(46, 211, 183, 30));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Inside);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn piano_input(
        &mut self,
        ui: &Ui,
        resp: &egui::Response,
        ch: ChannelId,
        rect: Rect,
        keys: Rect,
        ruler: Rect,
        grid: Rect,
        lane: Rect,
    ) {
        let (pos, p1, p2, pm, down1, down2, downm, released, mods, scroll, delta) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.secondary_pressed(),
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.primary_down(),
                i.pointer.secondary_down(),
                i.pointer.middle_down(),
                i.pointer.any_released(),
                i.modifiers,
                i.smooth_scroll_delta,
                i.pointer.delta(),
            )
        });
        let hovered = resp.hovered() || resp.dragged();

        // Wheel: scroll vertically, Shift = horizontal, Ctrl = zoom around the pointer.
        if hovered && (scroll.x != 0.0 || scroll.y != 0.0) {
            let pr = &mut self.piano;
            if mods.ctrl {
                if let Some(p) = pos {
                    let v =
                        View { grid, px: pr.px_per_tick, scroll: pr.scroll_tick, top_key: pr.top_key, row_h: pr.row_h };
                    let t = v.tick(p.x);
                    pr.px_per_tick = (pr.px_per_tick * 1.0015f32.powf(scroll.y)).clamp(0.005, 2.0);
                    pr.scroll_tick = (t - (p.x - grid.left()) / pr.px_per_tick).max(0.0);
                }
            } else if mods.shift {
                pr.scroll_tick = (pr.scroll_tick - (scroll.y + scroll.x) / pr.px_per_tick).max(0.0);
            } else {
                pr.top_key = (pr.top_key + scroll.y / pr.row_h).clamp(12.0, 131.0);
                pr.scroll_tick = (pr.scroll_tick - scroll.x / pr.px_per_tick).max(0.0);
            }
        }

        let v = View {
            grid,
            px: self.piano.px_per_tick,
            scroll: self.piano.scroll_tick,
            top_key: self.piano.top_key,
            row_h: self.piano.row_h,
        };

        // --- gesture start ---
        if let Some(p) = pos
            && resp.hovered()
            && self.piano.drag.is_none()
            && rect.contains(p)
        {
            if pm {
                self.begin(DragKind::Pan, "");
            } else if p1 && ruler.contains(p) {
                let t = snap_floor(v.tick(p.x).max(0.0) as Tick, TICKS_PER_STEP);
                self.session.send(EngineCmd::Seek(t));
            } else if p1 && keys.contains(p) {
                let key = v.key(p.y).clamp(0, 127) as u8;
                self.preview_on(ch, key, 0.8);
                self.begin(DragKind::Keyboard, "");
            } else if p1 && lane.contains(p) {
                self.begin(DragKind::Velocity, "Velocity");
            } else if grid.contains(p) && (p1 || p2) {
                self.grid_press(ch, v, p, p1, mods);
            }
        }

        // --- gesture update ---
        if let Some(p) = pos {
            self.drag_update(ch, v, lane, p, delta, mods, down1 || down2 || downm);
        }

        // --- gesture end ---
        if released && !(down1 || down2 || downm) {
            self.end_drag(ch, v, pos, mods);
        }
    }

    fn begin(&mut self, kind: DragKind, label: &'static str) {
        self.piano.drag = Some(Drag { kind, before: self.project.clone(), changed: false, label });
    }

    fn hit_note(&self, ch: ChannelId, v: View, p: Pos2) -> Option<usize> {
        let notes = self.project.current().notes(ch);
        (0..notes.len()).rev().find(|&i| v.note_rect(&notes[i]).contains(p))
    }

    fn snapped(&self, tick: f32, alt: bool) -> Tick {
        let t = tick.max(0.0) as Tick;
        if alt { t } else { snap_floor(t, self.snap) }
    }

    fn grid_press(&mut self, ch: ChannelId, v: View, p: Pos2, primary: bool, mods: egui::Modifiers) {
        let hit = self.hit_note(ch, v, p);
        let tick = v.tick(p.x);
        let key = v.key(p.y).clamp(0, 127);
        if !primary {
            if self.piano.tool == Tool::Zoom {
                self.zoom_at(v, p.x, 1.0 / 1.5);
                return;
            }
            // Right button deletes (FL default), continuing while held.
            self.begin(DragKind::Erase, "Delete notes");
            self.erase_at(ch, v, p);
            return;
        }
        match self.piano.tool {
            Tool::Draw | Tool::Paint if mods.ctrl => {
                self.begin(DragKind::Select { start: p, additive: mods.shift }, "");
            }
            Tool::Draw | Tool::Paint | Tool::Select => {
                if let Some(i) = hit {
                    let note = self.project.current().notes(ch)[i];
                    let near_edge = v.note_rect(&note).right() - p.x < EDGE_PX;
                    let before = self.project.clone();
                    let mut changed = false;
                    let notes = self.pr_notes(ch);
                    if !notes[i].selected {
                        for n in notes.iter_mut() {
                            n.selected = false;
                        }
                        notes[i].selected = true;
                    }
                    let mut lead = i;
                    if mods.shift && !near_edge {
                        // Shift+click clones the selection; the clones are what moves.
                        let clones: Vec<Note> = notes.iter().filter(|n| n.selected).copied().collect();
                        for n in notes.iter_mut() {
                            n.selected = false;
                        }
                        let base = notes.len();
                        let lead_note = notes[i];
                        notes.extend(clones.iter().copied().map(|mut n| {
                            n.selected = true;
                            n
                        }));
                        lead = base
                            + clones
                                .iter()
                                .position(|c| c.start == lead_note.start && c.key == lead_note.key)
                                .unwrap_or(0);
                        changed = true;
                    }
                    let originals: Vec<(usize, Note)> =
                        notes.iter().enumerate().filter(|(_, n)| n.selected).map(|(j, n)| (j, *n)).collect();
                    let kind = if near_edge {
                        DragKind::Resize { anchor_tick: tick, originals, lead }
                    } else {
                        DragKind::Move { anchor_tick: tick, anchor_key: key, originals, lead }
                    };
                    let label = if near_edge {
                        "Resize notes"
                    } else if changed {
                        "Clone notes"
                    } else {
                        "Move notes"
                    };
                    self.piano.drag = Some(Drag { kind, before, changed, label });
                    self.preview_on(ch, note.key, note.velocity);
                } else if self.piano.tool == Tool::Select {
                    self.begin(DragKind::Select { start: p, additive: mods.shift }, "");
                } else {
                    let start = self.snapped(tick, mods.alt);
                    let len = self.piano.last_len.max(1);
                    let before = self.project.clone();
                    let notes = self.pr_notes(ch);
                    for n in notes.iter_mut() {
                        n.selected = false;
                    }
                    let mut n = Note::new(start, len, key as u8);
                    n.selected = true;
                    notes.push(n);
                    let idx = notes.len() - 1;
                    let kind = if self.piano.tool == Tool::Paint {
                        DragKind::Paint {
                            next_tick: start + len.max(if self.snap > 0 { self.snap } else { TICKS_PER_STEP / 4 }),
                        }
                    } else {
                        DragKind::Move { anchor_tick: tick, anchor_key: key, originals: vec![(idx, n)], lead: idx }
                    };
                    let label = if self.piano.tool == Tool::Paint { "Paint notes" } else { "Add note" };
                    self.piano.drag = Some(Drag { kind, before, changed: true, label });
                    self.needs_sync = true;
                    self.preview_on(ch, key as u8, n.velocity);
                }
            }
            Tool::Delete => {
                self.begin(DragKind::Erase, "Delete notes");
                self.erase_at(ch, v, p);
            }
            Tool::Mute => {
                if let Some(i) = hit {
                    self.checkpoint("Mute note");
                    let n = &mut self.pr_notes(ch)[i];
                    n.muted = !n.muted;
                }
            }
            Tool::Slice => {
                if let Some(i) = hit {
                    let at = self.snapped(tick, mods.alt);
                    let n = self.project.current().notes(ch)[i];
                    if at > n.start && at < n.end() {
                        self.checkpoint("Slice note");
                        let notes = self.pr_notes(ch);
                        notes[i].length = at - n.start;
                        let mut right = n;
                        right.start = at;
                        right.length = n.end() - at;
                        notes.push(right);
                    }
                }
            }
            Tool::Zoom => self.zoom_at(v, p.x, 1.5),
        }
    }

    fn zoom_at(&mut self, v: View, x: f32, factor: f32) {
        let t = v.tick(x);
        let pr = &mut self.piano;
        pr.px_per_tick = (pr.px_per_tick * factor).clamp(0.005, 2.0);
        pr.scroll_tick = (t - (x - v.grid.left()) / pr.px_per_tick).max(0.0);
    }

    fn erase_at(&mut self, ch: ChannelId, v: View, p: Pos2) {
        if let Some(i) = self.hit_note(ch, v, p) {
            self.pr_notes(ch).remove(i);
            if let Some(d) = &mut self.piano.drag {
                d.changed = true;
            }
            self.needs_sync = true;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn drag_update(
        &mut self,
        ch: ChannelId,
        v: View,
        lane: Rect,
        p: Pos2,
        delta: egui::Vec2,
        mods: egui::Modifiers,
        any_down: bool,
    ) {
        if !any_down {
            return;
        }
        let Some(mut drag) = self.piano.drag.take() else { return };
        let snap = if mods.alt { 0 } else { self.snap };
        match &mut drag.kind {
            DragKind::Pan => {
                self.piano.scroll_tick = (self.piano.scroll_tick - delta.x / self.piano.px_per_tick).max(0.0);
                self.piano.top_key = (self.piano.top_key + delta.y / self.piano.row_h).clamp(12.0, 131.0);
            }
            DragKind::Move { anchor_tick, anchor_key, originals, lead } => {
                let lead_orig = originals.iter().find(|(i, _)| i == lead).map(|o| o.1).unwrap_or(originals[0].1);
                let raw = lead_orig.start as f32 + (v.tick(p.x) - *anchor_tick);
                let new_start = if snap > 0 { snap_round(raw.max(0.0) as Tick, snap) } else { raw.max(0.0) as Tick };
                let min_start = originals.iter().map(|o| o.1.start).min().unwrap_or(0);
                let dt = (new_start - lead_orig.start).max(-min_start);
                let (kmin, kmax) =
                    originals.iter().fold((127i32, 0i32), |(a, b), o| (a.min(o.1.key as i32), b.max(o.1.key as i32)));
                let dk = (v.key(p.y) - *anchor_key).clamp(-kmin, 127 - kmax);
                let notes = self.project.current_mut().notes_mut(ch);
                let mut moved = false;
                for (i, o) in originals.iter() {
                    if let Some(n) = notes.get_mut(*i) {
                        let (s, k) = (o.start + dt, (o.key as i32 + dk) as u8);
                        if n.start != s || n.key != k {
                            moved = true;
                        }
                        n.start = s;
                        n.key = k;
                    }
                }
                if moved {
                    drag.changed = true;
                    self.needs_sync = true;
                    let k = (lead_orig.key as i32 + dk) as u8;
                    if self.preview.map(|(_, pk)| pk) != Some(k) {
                        self.preview_on(ch, k, lead_orig.velocity);
                    }
                }
            }
            DragKind::Resize { anchor_tick, originals, lead } => {
                let lead_orig = originals.iter().find(|(i, _)| i == lead).map(|o| o.1).unwrap_or(originals[0].1);
                let raw_end = lead_orig.end() as f32 + (v.tick(p.x) - *anchor_tick);
                let new_end = if snap > 0 { snap_round(raw_end as Tick, snap) } else { raw_end as Tick };
                let dl = new_end - lead_orig.end();
                let min_len = if snap > 0 { snap } else { PPQ / 96 };
                let notes = self.project.current_mut().notes_mut(ch);
                for (i, o) in originals.iter() {
                    if let Some(n) = notes.get_mut(*i) {
                        let l = (o.length + dl).max(min_len);
                        if n.length != l {
                            drag.changed = true;
                            self.needs_sync = true;
                        }
                        n.length = l;
                    }
                }
                self.piano.last_len = (lead_orig.length + dl).max(min_len);
            }
            DragKind::Erase => {
                if let Some(i) = self.hit_note(ch, v, p) {
                    self.pr_notes(ch).remove(i);
                    drag.changed = true;
                    self.needs_sync = true;
                }
            }
            DragKind::Paint { next_tick } => {
                let t = v.tick(p.x).max(0.0) as Tick;
                let len = self.piano.last_len.max(1);
                // Paint advances by at least one snap unit (or 1/4 step without snap), so a
                // tiny last-used length can't flood the pattern with overlapping notes.
                let stride = len.max(if snap > 0 { snap } else { TICKS_PER_STEP / 4 });
                let key = v.key(p.y).clamp(0, 127) as u8;
                while t >= *next_tick {
                    let mut n = Note::new(*next_tick, len, key);
                    n.selected = true;
                    self.pr_notes(ch).push(n);
                    *next_tick += stride;
                    drag.changed = true;
                    self.needs_sync = true;
                }
            }
            DragKind::Velocity => {
                let vel = (1.0 - (p.y - lane.top()) / lane.height()).clamp(0.0, 1.0);
                let t = v.tick(p.x);
                let tol = 4.0 / v.px;
                let notes = self.project.current_mut().notes_mut(ch);
                let any_sel = notes.iter().any(|n| n.selected);
                for n in notes.iter_mut().filter(|n| !any_sel || n.selected) {
                    if t >= n.start as f32 - tol && t <= (n.start as f32 + (n.length as f32).max(tol)) {
                        n.velocity = vel;
                        drag.changed = true;
                        self.needs_sync = true;
                    }
                }
            }
            DragKind::Select { .. } | DragKind::Keyboard => {}
        }
        self.piano.drag = Some(drag);
    }

    fn end_drag(&mut self, ch: ChannelId, v: View, pos: Option<Pos2>, _mods: egui::Modifiers) {
        let Some(drag) = self.piano.drag.take() else { return };
        self.preview_off();
        if let (DragKind::Select { start, additive }, Some(p)) = (&drag.kind, pos) {
            let r = Rect::from_two_pos(*start, p);
            let additive = *additive;
            let notes = self.pr_notes(ch);
            for n in notes.iter_mut() {
                let hit = v.note_rect(n).intersects(r);
                n.selected = if additive { n.selected || hit } else { hit };
            }
            return;
        }
        if drag.changed {
            self.pr_notes(ch).sort_by_key(|n| (n.start, n.key));
            self.checkpoint_from(&drag.before, drag.label);
        }
    }

    // ----- drawing -------------------------------------------------------

    fn draw_grid(&self, painter: &egui::Painter, v: View, ch: ChannelId) {
        let g = v.grid;
        painter.rect_filled(g, 0.0, theme::GRID_BG);
        let k_top = v.key(g.top()).min(127);
        let k_bottom = v.key(g.bottom()).max(0);
        for k in k_bottom..=k_top {
            let y = v.y(k);
            let row = Rect::from_min_max(pos2(g.left(), y), pos2(g.right(), y + v.row_h));
            if is_black(k) {
                painter.rect_filled(row, 0.0, theme::GRID_BLACK_KEY);
            }
            if k.rem_euclid(12) == 0 {
                painter.hline(g.x_range(), y + v.row_h, Stroke::new(1.0, theme::GRID_BEAT));
            }
        }
        let ts = self.project.time_signature;
        let (step, beat, bar) = (TICKS_PER_STEP, ts.ticks_per_beat(), ts.ticks_per_bar());
        let t0 = snap_floor(v.tick(g.left()).max(0.0) as Tick, step);
        let t1 = v.tick(g.right()) as Tick;
        let draw_steps = step as f32 * v.px > 6.0;
        let mut t = t0;
        while t <= t1 {
            let x = v.x(t as f32);
            let col = if t % bar == 0 {
                Some(theme::GRID_BAR)
            } else if t % beat == 0 {
                Some(theme::GRID_BEAT)
            } else if draw_steps {
                Some(theme::GRID_LINE)
            } else {
                None
            };
            if let Some(c) = col {
                painter.vline(x, g.y_range(), Stroke::new(1.0, c));
            }
            t += step;
        }
        // Shade beyond the pattern end.
        let end_x = v.x(self.project.current().length_ticks(ts) as f32);
        if end_x < g.right() {
            painter.rect_filled(
                Rect::from_min_max(pos2(end_x.max(g.left()), g.top()), g.right_bottom()),
                0.0,
                Color32::from_black_alpha(60),
            );
        }

        // Ghost notes of other channels.
        if self.piano.ghost {
            for pc in &self.project.current().channels {
                if pc.channel == ch {
                    continue;
                }
                for n in &pc.notes {
                    painter.rect_filled(v.note_rect(n), 2.0, Color32::from_white_alpha(25));
                }
            }
        }

        let color = self.project.channel(ch).map(|c| theme::rgb(c.color)).unwrap_or(theme::ACCENT);
        let font = FontId::proportional((v.row_h - 4.0).clamp(7.0, 11.0));
        for n in self.project.current().notes(ch) {
            let r = v.note_rect(n);
            if !r.intersects(g) {
                continue;
            }
            let fill = if n.muted {
                Color32::from_gray(90)
            } else if n.selected {
                color.lerp_to_gamma(Color32::WHITE, 0.45)
            } else {
                color.gamma_multiply(0.55 + 0.45 * n.velocity)
            };
            painter.rect_filled(r, 2.0, fill);
            let border = if n.selected { Color32::WHITE } else { Color32::from_black_alpha(140) };
            painter.rect_stroke(r, 2.0, Stroke::new(1.0, border), StrokeKind::Inside);
            if r.width() > 26.0 && v.row_h >= 10.0 {
                painter.text(
                    r.left_center() + vec2(3.0, 0.0),
                    Align2::LEFT_CENTER,
                    note_name(n.key as i32),
                    font.clone(),
                    Color32::from_black_alpha(200),
                );
            }
        }

        // Playhead.
        if self.shared.playing.load(std::sync::atomic::Ordering::Relaxed) {
            let x = v.x(self.shared.position_tick() as f32);
            if g.x_range().contains(x) {
                painter.vline(x, g.y_range(), Stroke::new(1.5, theme::PLAYHEAD));
            }
        }
    }

    fn draw_keyboard(&self, painter: &egui::Painter, v: View, r: Rect) {
        painter.rect_filled(r, 0.0, Color32::from_gray(230));
        let k_top = v.key(r.top()).min(127);
        let k_bottom = v.key(r.bottom()).max(0);
        let pressed = self.preview.map(|(_, k)| k as i32);
        for k in k_bottom..=k_top {
            let y = v.y(k);
            let row = Rect::from_min_max(pos2(r.left(), y), pos2(r.right(), y + v.row_h)).intersect(r);
            let mut fill = if is_black(k) { Color32::from_gray(30) } else { Color32::from_gray(232) };
            if pressed == Some(k) {
                fill = theme::ACCENT;
            }
            let row = if is_black(k) {
                Rect::from_min_max(row.min, pos2(r.left() + r.width() * 0.62, row.max.y))
            } else {
                row
            };
            painter.rect_filled(row, 0.0, fill);
            painter.hline(r.x_range(), y + v.row_h, Stroke::new(0.5, Color32::from_gray(150)));
            if k.rem_euclid(12) == 0 && v.row_h >= 8.0 {
                painter.text(
                    pos2(r.right() - 3.0, y + v.row_h / 2.0),
                    Align2::RIGHT_CENTER,
                    note_name(k),
                    FontId::proportional(9.0),
                    Color32::from_gray(60),
                );
            }
        }
    }

    fn draw_ruler(&self, painter: &egui::Painter, v: View, r: Rect) {
        painter.rect_filled(r, 0.0, theme::PANEL_DARK);
        let bar = self.project.time_signature.ticks_per_bar();
        let t0 = snap_floor(v.tick(r.left()).max(0.0) as Tick, bar);
        let t1 = v.tick(r.right()) as Tick;
        let skip = ((40.0 / (bar as f32 * v.px)).ceil() as i64).max(1);
        let mut t = t0;
        while t <= t1 {
            let barno = t / bar + 1;
            if (barno - 1) % skip == 0 {
                let x = v.x(t as f32);
                painter.vline(x, r.y_range(), Stroke::new(1.0, theme::GRID_BAR));
                painter.text(
                    pos2(x + 3.0, r.center().y),
                    Align2::LEFT_CENTER,
                    barno.to_string(),
                    FontId::proportional(11.0),
                    theme::TEXT,
                );
            }
            t += bar;
        }
        let x = v.x(self.shared.position_tick() as f32);
        if r.x_range().contains(x) {
            painter.add(egui::Shape::convex_polygon(
                vec![pos2(x - 5.0, r.top()), pos2(x + 5.0, r.top()), pos2(x, r.bottom())],
                theme::PLAYHEAD,
                Stroke::NONE,
            ));
        }
    }

    fn draw_velocity_lane(&self, painter: &egui::Painter, v: View, lane: Rect, ch: ChannelId) {
        painter.rect_filled(lane, 0.0, theme::PANEL_DARK);
        painter.text(
            pos2(lane.left() - KEYS_W + 4.0, lane.top() + 2.0),
            Align2::LEFT_TOP,
            "Velocity",
            FontId::proportional(10.0),
            theme::TEXT_DIM,
        );
        let color = self.project.channel(ch).map(|c| theme::rgb(c.color)).unwrap_or(theme::ACCENT);
        for n in self.project.current().notes(ch) {
            let x = v.x(n.start as f32);
            if !lane.x_range().contains(x) {
                continue;
            }
            let top = lane.bottom() - n.velocity * (lane.height() - 4.0);
            let c = if n.selected { Color32::WHITE } else { color };
            painter.vline(x + 1.0, top..=lane.bottom(), Stroke::new(2.0, c));
            painter.circle_filled(pos2(x + 1.0, top), 3.0, c);
        }
    }

    // ----- commands ------------------------------------------------------

    /// Indices of selected notes, or of all notes when nothing is selected (FL behaviour
    /// for transpose/quantize/legato/copy).
    fn sel_or_all(&self, ch: ChannelId) -> Vec<usize> {
        let notes = self.project.current().notes(ch);
        let sel: Vec<usize> = (0..notes.len()).filter(|&i| notes[i].selected).collect();
        if sel.is_empty() { (0..notes.len()).collect() } else { sel }
    }

    fn edit_notes(&mut self, ch: ChannelId, label: &str, f: impl FnOnce(&mut Vec<Note>, &[usize])) {
        let idx = self.sel_or_all(ch);
        if idx.is_empty() {
            self.set_status("No notes.");
            return;
        }
        self.checkpoint(label);
        let notes = self.pr_notes(ch);
        f(notes, &idx);
        notes.retain(|n| n.length > 0);
        notes.sort_by_key(|n| (n.start, n.key));
    }

    pub fn piano_command(&mut self, cmd: Command) -> bool {
        use Command::*;
        let Some(ch) = self.selected_channel else { return false };
        let step = self.snap_or(TICKS_PER_STEP);
        match cmd {
            SelectAll | Deselect | InvertSelection => {
                for n in self.pr_notes(ch).iter_mut() {
                    n.selected = match cmd {
                        SelectAll => true,
                        Deselect => false,
                        _ => !n.selected,
                    };
                }
            }
            DeleteSelection => {
                if self.project.current().notes(ch).iter().any(|n| n.selected) {
                    self.checkpoint("Delete notes");
                    self.pr_notes(ch).retain(|n| !n.selected);
                }
            }
            Copy | Cut => {
                let idx = self.sel_or_all(ch);
                let notes = self.project.current().notes(ch);
                self.piano.clipboard = idx.iter().map(|&i| notes[i]).collect();
                if cmd == Cut && !idx.is_empty() {
                    self.checkpoint("Cut notes");
                    let notes = self.pr_notes(ch);
                    let mut i = 0;
                    notes.retain(|_| {
                        let keep = !idx.contains(&i);
                        i += 1;
                        keep
                    });
                }
                self.set_status(format!("{} notes copied.", self.piano.clipboard.len()));
            }
            Paste => {
                if self.piano.clipboard.is_empty() {
                    self.set_status("Clipboard is empty.");
                } else {
                    self.checkpoint("Paste notes");
                    let clip = self.piano.clipboard.clone();
                    let notes = self.pr_notes(ch);
                    for n in notes.iter_mut() {
                        n.selected = false;
                    }
                    notes.extend(clip.into_iter().map(|mut n| {
                        n.selected = true;
                        n
                    }));
                    notes.sort_by_key(|n| (n.start, n.key));
                }
            }
            DuplicateRight => {
                let beat = self.project.time_signature.ticks_per_beat();
                self.edit_notes(ch, "Duplicate notes", |notes, idx| {
                    let start = idx.iter().map(|&i| notes[i].start).min().unwrap_or(0);
                    let end = idx.iter().map(|&i| notes[i].end()).max().unwrap_or(0);
                    let start_b = snap_floor(start, beat);
                    let span = ((end - start_b + beat - 1) / beat).max(1) * beat;
                    let copies: Vec<Note> = idx.iter().map(|&i| notes[i]).collect();
                    for n in notes.iter_mut() {
                        n.selected = false;
                    }
                    notes.extend(copies.into_iter().map(|mut n| {
                        n.start += span;
                        n.selected = true;
                        n
                    }));
                });
            }
            MoveSelectionLeft | MoveSelectionRight | NudgeLeft | NudgeRight => {
                let amount = if matches!(cmd, NudgeLeft | NudgeRight) { (step / 4).max(1) } else { step };
                let d = if matches!(cmd, MoveSelectionLeft | NudgeLeft) { -amount } else { amount };
                self.edit_notes(ch, "Move notes", |notes, idx| {
                    let min = idx.iter().map(|&i| notes[i].start).min().unwrap_or(0);
                    let d = d.max(-min);
                    for &i in idx {
                        notes[i].start += d;
                    }
                });
            }
            MoveSelectionUp | MoveSelectionDown | NudgeUp | NudgeDown | TransposeOctaveUp | TransposeOctaveDown => {
                let d: i32 = match cmd {
                    MoveSelectionUp | NudgeUp => 1,
                    MoveSelectionDown | NudgeDown => -1,
                    TransposeOctaveUp => 12,
                    _ => -12,
                };
                self.edit_notes(ch, "Transpose notes", |notes, idx| {
                    let (lo, hi) = idx
                        .iter()
                        .fold((127, 0), |(a, b), &i| (a.min(notes[i].key as i32), b.max(notes[i].key as i32)));
                    if lo + d < 0 || hi + d > 127 {
                        return;
                    }
                    for &i in idx {
                        notes[i].key = (notes[i].key as i32 + d) as u8;
                    }
                });
            }
            MuteSelection | UnmuteSelection => {
                let mute = cmd == MuteSelection;
                self.edit_notes(ch, if mute { "Mute notes" } else { "Unmute notes" }, |notes, idx| {
                    for &i in idx {
                        notes[i].muted = mute;
                    }
                });
            }
            PrQuickLegato => self.edit_notes(ch, "Quick legato", |notes, idx| {
                let mut starts: Vec<Tick> = idx.iter().map(|&i| notes[i].start).collect();
                starts.sort();
                starts.dedup();
                for &i in idx {
                    if let Some(&next) = starts.iter().find(|&&s| s > notes[i].start) {
                        notes[i].length = next - notes[i].start;
                    }
                }
            }),
            PrQuickQuantize => self.edit_notes(ch, "Quick quantize", |notes, idx| {
                for &i in idx {
                    notes[i].start = snap_round(notes[i].start, step).max(0);
                }
            }),
            PrQuantize => self.win.quantize = true,
            PrToggleGhostChannels => self.piano.ghost = !self.piano.ghost,
            ZoomIn => self.piano.px_per_tick = (self.piano.px_per_tick * 1.25).min(2.0),
            ZoomOut => self.piano.px_per_tick = (self.piano.px_per_tick / 1.25).max(0.005),
            ZoomAll => {
                let len = self.project.current().length_ticks(self.project.time_signature) as f32;
                self.piano.px_per_tick = (self.piano.last_grid_w / len).clamp(0.005, 2.0);
                self.piano.scroll_tick = 0.0;
            }
            CenterOnPlayhead => {
                let t = self.shared.position_tick() as f32;
                self.piano.scroll_tick = (t - self.piano.last_grid_w / 2.0 / self.piano.px_per_tick).max(0.0);
            }
            SelectByColor => {
                let colors: Vec<u8> =
                    self.project.current().notes(ch).iter().filter(|n| n.selected).map(|n| n.color).collect();
                for n in self.pr_notes(ch).iter_mut() {
                    n.selected = colors.contains(&n.color);
                }
            }
            PrChannelAbove | PrChannelBelow | PrNextChannel => {
                let d = if cmd == PrChannelAbove { -1 } else { 1 };
                self.step_channel(d, false);
            }
            PrChannelAboveWithNotes | PrChannelBelowWithNotes | PrNextChannelWithNotes => {
                let d = if cmd == PrChannelAboveWithNotes { -1 } else { 1 };
                self.step_channel(d, true);
            }
            _ => return false,
        }
        self.needs_sync = true;
        true
    }

    pub fn step_channel(&mut self, d: i32, with_notes: bool) {
        let n = self.project.channels.len() as i32;
        if n == 0 {
            return;
        }
        let cur = self.selected_channel.and_then(|c| self.project.channels.iter().position(|x| x.id == c)).unwrap_or(0)
            as i32;
        for k in 1..=n {
            let i = (cur + d * k).rem_euclid(n) as usize;
            let id = self.project.channels[i].id;
            if !with_notes || !self.project.current().notes(id).is_empty() {
                self.select_channel(id, false);
                return;
            }
        }
    }

    pub fn quantize_window(&mut self, ui: &mut Ui) {
        ui.label("Quantize the selected notes (all notes if none are selected) to the current snap.");
        ui.add(egui::Slider::new(&mut self.quantize_strength, 0.0..=1.0).text("Strength"));
        ui.checkbox(&mut self.quantize_ends, "Quantize note ends");
        if ui.button("Apply").clicked()
            && let Some(ch) = self.selected_channel
        {
            let (strength, ends, step) = (self.quantize_strength, self.quantize_ends, self.snap_or(TICKS_PER_STEP));
            self.edit_notes(ch, "Quantize", |notes, idx| {
                let pull = |t: Tick| (t as f32 + (snap_round(t, step) - t) as f32 * strength).round() as Tick;
                for &i in idx {
                    let n = &mut notes[i];
                    let end = n.end();
                    n.start = pull(n.start).max(0);
                    if ends {
                        n.length = (pull(end) - n.start).max(1);
                    }
                }
            });
            self.needs_sync = true;
        }
    }
}
