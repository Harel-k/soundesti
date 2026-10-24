//! Playlist window: pattern clips on tracks, FL-style picker panel, tools and editing
//! commands. Same gesture model as the piano roll: press starts a gesture, release
//! commits one undo step, Escape cancels.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use soundesti_audio::Command as EngineCmd;
use soundesti_commands::Command;
use soundesti_project::{Clip, PatternId, Project};
use soundesti_timeline::{Tick, snap_floor, snap_round};

use crate::app::{App, Tool};
use crate::{icons, theme};

const PICKER_W: f32 = 150.0;
const HEADER_W: f32 = 120.0;
const RULER_H: f32 = 22.0;
const EDGE_PX: f32 = 7.0;

pub struct PlaylistView {
    pub tool: Tool,
    pub px_per_tick: f32,
    pub scroll_tick: f32,
    pub scroll_track: f32,
    pub track_h: f32,
    pub clipboard: Vec<Clip>,
    last_w: f32,
    drag: Option<Drag>,
}

impl Default for PlaylistView {
    fn default() -> Self {
        Self {
            tool: Tool::Draw,
            px_per_tick: 0.03,
            scroll_tick: 0.0,
            scroll_track: 0.0,
            track_h: 36.0,
            clipboard: Vec::new(),
            last_w: 900.0,
            drag: None,
        }
    }
}

struct Drag {
    kind: Kind,
    before: Project,
    changed: bool,
    label: &'static str,
}

enum Kind {
    Move { anchor_tick: f32, anchor_track: i32, originals: Vec<(usize, Clip)>, lead: usize },
    Resize { anchor_tick: f32, originals: Vec<(usize, Clip)>, lead: usize },
    Select { start: Pos2, additive: bool },
    Erase,
    Paint { track: u16, next: Tick },
    Pan,
}

impl PlaylistView {
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
    top: f32,
    th: f32,
}

impl View {
    fn x(&self, t: f32) -> f32 {
        self.grid.left() + (t - self.scroll) * self.px
    }
    fn tick(&self, x: f32) -> f32 {
        self.scroll + (x - self.grid.left()) / self.px
    }
    fn y(&self, track: f32) -> f32 {
        self.grid.top() + (track - self.top) * self.th
    }
    fn track(&self, y: f32) -> i32 {
        (self.top + (y - self.grid.top()) / self.th).floor() as i32
    }
    fn clip_rect(&self, c: &Clip) -> Rect {
        let y = self.y(c.track as f32);
        Rect::from_min_max(
            pos2(self.x(c.start as f32), y + 1.0),
            pos2(self.x(c.end() as f32).max(self.x(c.start as f32) + 4.0), y + self.th - 1.0),
        )
    }
}

fn pattern_color(project: &Project, id: PatternId) -> Color32 {
    project.pattern(id).and_then(|p| p.color).map(theme::rgb).unwrap_or(Color32::from_rgb(0x5a, 0x9b, 0xd5))
}

impl App {
    /// Playlist snap: the global snap, but never finer than a beat (FL's playlist default
    /// is coarser than the piano roll's).
    fn pl_snap(&self) -> Tick {
        let beat = self.project.time_signature.ticks_per_beat();
        if self.snap == 0 { 0 } else { self.snap.max(beat) }
    }

    pub fn playlist_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            for t in Tool::ALL {
                if icons::button(ui, t.icon(), self.playlist.tool == t, t.label()).clicked() {
                    self.playlist.tool = t;
                }
            }
            ui.separator();
            let song = self.project.song_mode;
            if icons::text_button(ui, icons::Icon::Song, if song { "Song mode" } else { "Pattern mode" }, song)
                .on_hover_text("Switch pattern/song mode (L). Song mode plays this arrangement.")
                .clicked()
            {
                self.run(&ui.ctx().clone(), Command::ToggleSongMode, None);
            }
            let n = self.project.playlist.clips.len();
            let sel = self.project.playlist.clips.iter().filter(|c| c.selected).count();
            let len = soundesti_timeline::BarStepTick::from_tick(
                self.project.playlist.song_length(self.project.time_signature),
                self.project.time_signature,
            )
            .bar - 1;
            ui.label(
                RichText::new(format!("{n} clips, {sel} selected · song length {len} bars"))
                    .small()
                    .color(theme::TEXT_DIM),
            );
        });
        ui.horizontal_top(|ui| {
            self.pattern_picker(ui);
            self.playlist_canvas(ui);
        });
    }

    /// FL's picker panel: click a pattern to make it the one you draw with.
    fn pattern_picker(&mut self, ui: &mut Ui) {
        let h = ui.available_height().max(200.0);
        egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(4).show(ui, |ui| {
            ui.set_width(PICKER_W - 8.0);
            ui.set_height(h - 8.0);
            ui.label(RichText::new("Patterns").small().color(theme::TEXT_DIM));
            egui::ScrollArea::vertical().id_salt("pl_picker").show(ui, |ui| {
                let list: Vec<(PatternId, String)> =
                    self.project.patterns.iter().map(|p| (p.id, p.name.clone())).collect();
                for (id, name) in list {
                    let selected = self.project.current_pattern == id;
                    let color = pattern_color(&self.project, id);
                    let (rect, resp) = ui.allocate_exact_size(vec2(PICKER_W - 16.0, 22.0), Sense::click());
                    ui.painter().rect_filled(
                        rect,
                        3.0,
                        if selected { color.gamma_multiply(0.8) } else { color.gamma_multiply(0.35) },
                    );
                    ui.painter().text(
                        rect.left_center() + vec2(6.0, 0.0),
                        Align2::LEFT_CENTER,
                        &name,
                        FontId::proportional(12.0),
                        if selected { Color32::BLACK } else { theme::TEXT },
                    );
                    if resp.clicked() {
                        self.project.current_pattern = id;
                        self.needs_sync = true;
                    }
                    if resp.double_clicked() {
                        self.win.channel_rack = true;
                        self.focus = soundesti_shortcuts::Context::ChannelRack;
                    }
                }
                if ui.small_button("New pattern").clicked() {
                    self.run(&ui.ctx().clone(), Command::NextEmptyPattern, None);
                }
            });
        });
    }

    fn playlist_canvas(&mut self, ui: &mut Ui) {
        let size = ui.available_size().max(vec2(400.0, 200.0));
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let headers =
            Rect::from_min_max(pos2(rect.left(), rect.top() + RULER_H), pos2(rect.left() + HEADER_W, rect.bottom()));
        let ruler =
            Rect::from_min_max(pos2(rect.left() + HEADER_W, rect.top()), pos2(rect.right(), rect.top() + RULER_H));
        let grid = Rect::from_min_max(pos2(rect.left() + HEADER_W, rect.top() + RULER_H), rect.right_bottom());
        self.playlist.last_w = grid.width();
        self.playlist_input(ui, &resp, rect, headers, ruler, grid);
        let v = View {
            grid,
            px: self.playlist.px_per_tick,
            scroll: self.playlist.scroll_tick,
            top: self.playlist.scroll_track,
            th: self.playlist.track_h,
        };
        let painter = ui.painter_at(rect);
        self.draw_playlist_grid(&painter, v);
        self.draw_track_headers(ui, &painter, v, headers);
        self.draw_playlist_ruler(&painter, v, ruler);
        if let Some(Drag { kind: Kind::Select { start, .. }, .. }) = &self.playlist.drag
            && let Some(p) = ui.input(|i| i.pointer.interact_pos())
        {
            let r = Rect::from_two_pos(*start, p);
            painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(46, 211, 183, 30));
            painter.rect_stroke(r, 0.0, Stroke::new(1.0, theme::ACCENT), StrokeKind::Inside);
        }
    }

    fn playlist_input(&mut self, ui: &Ui, resp: &egui::Response, rect: Rect, headers: Rect, ruler: Rect, grid: Rect) {
        let (pos, p1, p2, pm, down, released, mods, scroll, delta, dbl) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_pressed(),
                i.pointer.secondary_pressed(),
                i.pointer.button_pressed(egui::PointerButton::Middle),
                i.pointer.any_down(),
                i.pointer.any_released(),
                i.modifiers,
                i.smooth_scroll_delta,
                i.pointer.delta(),
                i.pointer.button_double_clicked(egui::PointerButton::Primary),
            )
        });
        let hovered = resp.hovered() || resp.dragged();
        if hovered && (scroll.x != 0.0 || scroll.y != 0.0) {
            let pl = &mut self.playlist;
            if mods.ctrl {
                if let Some(p) = pos {
                    let t = pl.scroll_tick + (p.x - grid.left()) / pl.px_per_tick;
                    pl.px_per_tick = (pl.px_per_tick * 1.0015f32.powf(scroll.y)).clamp(0.002, 0.5);
                    pl.scroll_tick = (t - (p.x - grid.left()) / pl.px_per_tick).max(0.0);
                }
            } else if mods.shift {
                pl.scroll_tick = (pl.scroll_tick - (scroll.y + scroll.x) / pl.px_per_tick).max(0.0);
            } else {
                pl.scroll_track = (pl.scroll_track - scroll.y / pl.track_h)
                    .clamp(0.0, (self.project.playlist.tracks.len() as f32 - 1.0).max(0.0));
                pl.scroll_tick = (pl.scroll_tick - scroll.x / pl.px_per_tick).max(0.0);
            }
        }
        let v = View {
            grid,
            px: self.playlist.px_per_tick,
            scroll: self.playlist.scroll_tick,
            top: self.playlist.scroll_track,
            th: self.playlist.track_h,
        };

        if let Some(p) = pos
            && resp.hovered()
            && self.playlist.drag.is_none()
            && rect.contains(p)
        {
            if pm {
                self.pl_begin(Kind::Pan, "");
            } else if p1 && ruler.contains(p) {
                if !self.project.song_mode {
                    self.project.song_mode = true;
                    self.needs_sync = true;
                }
                let t = snap_floor(v.tick(p.x).max(0.0) as Tick, self.project.time_signature.ticks_per_beat());
                self.session.send(EngineCmd::Seek(t));
            } else if headers.contains(p) && p1 {
                let t = v.track(p.y);
                if t >= 0 && (t as usize) < self.project.playlist.tracks.len() && p.x < headers.left() + 22.0 {
                    self.checkpoint("Mute track");
                    let tr = &mut self.project.playlist.tracks[t as usize];
                    tr.muted = !tr.muted;
                }
            } else if grid.contains(p) && (p1 || p2) {
                if dbl && let Some(i) = self.pl_hit(v, p) {
                    // Double-click a clip: edit its pattern (FL opens it in the Channel Rack).
                    self.project.current_pattern = self.project.playlist.clips[i].pattern;
                    self.win.channel_rack = true;
                    self.needs_sync = true;
                } else {
                    self.pl_press(v, p, p1, mods);
                }
            }
        }
        if let Some(p) = pos
            && down
        {
            self.pl_update(v, p, delta, mods);
        }
        if released && !down {
            self.pl_end(v, pos);
        }
    }

    fn pl_begin(&mut self, kind: Kind, label: &'static str) {
        self.playlist.drag = Some(Drag { kind, before: self.project.clone(), changed: false, label });
    }

    fn pl_hit(&self, v: View, p: Pos2) -> Option<usize> {
        let clips = &self.project.playlist.clips;
        (0..clips.len()).rev().find(|&i| v.clip_rect(&clips[i]).contains(p))
    }

    fn pl_press(&mut self, v: View, p: Pos2, primary: bool, mods: egui::Modifiers) {
        let hit = self.pl_hit(v, p);
        let tick = v.tick(p.x);
        let track = v.track(p.y).max(0);
        if !primary {
            if self.playlist.tool == Tool::Zoom {
                self.pl_zoom(v, p.x, 1.0 / 1.5);
                return;
            }
            self.pl_begin(Kind::Erase, "Delete clips");
            self.pl_erase(v, p);
            return;
        }
        let snap = if mods.alt { 0 } else { self.pl_snap() };
        match self.playlist.tool {
            Tool::Draw | Tool::Paint if mods.ctrl => self.pl_begin(Kind::Select { start: p, additive: mods.shift }, ""),
            Tool::Draw | Tool::Paint | Tool::Select => {
                if let Some(i) = hit {
                    let c = self.project.playlist.clips[i];
                    let near_edge = v.clip_rect(&c).right() - p.x < EDGE_PX;
                    let before = self.project.clone();
                    let clips = &mut self.project.playlist.clips;
                    if !clips[i].selected {
                        clips.iter_mut().for_each(|c| c.selected = false);
                        clips[i].selected = true;
                    }
                    let mut lead = i;
                    let mut changed = false;
                    if mods.shift && !near_edge {
                        // Shift+drag clones the selection.
                        let clones: Vec<Clip> = clips.iter().filter(|c| c.selected).copied().collect();
                        clips.iter_mut().for_each(|c| c.selected = false);
                        let base = clips.len();
                        lead = base + clones.iter().position(|k| k.start == c.start && k.track == c.track).unwrap_or(0);
                        clips.extend(clones);
                        changed = true;
                    }
                    let originals: Vec<(usize, Clip)> =
                        clips.iter().enumerate().filter(|(_, c)| c.selected).map(|(j, c)| (j, *c)).collect();
                    let kind = if near_edge {
                        Kind::Resize { anchor_tick: tick, originals, lead }
                    } else {
                        Kind::Move { anchor_tick: tick, anchor_track: track, originals, lead }
                    };
                    let label = if near_edge {
                        "Resize clips"
                    } else if changed {
                        "Clone clips"
                    } else {
                        "Move clips"
                    };
                    self.playlist.drag = Some(Drag { kind, before, changed, label });
                    self.project.current_pattern = c.pattern;
                } else if self.playlist.tool == Tool::Select {
                    self.pl_begin(Kind::Select { start: p, additive: mods.shift }, "");
                } else if (track as usize) < self.project.playlist.tracks.len() {
                    let pid = self.project.current_pattern;
                    let len = self.project.current().length_ticks(self.project.time_signature);
                    let start = snap_floor(tick.max(0.0) as Tick, snap);
                    let before = self.project.clone();
                    let clips = &mut self.project.playlist.clips;
                    clips.iter_mut().for_each(|c| c.selected = false);
                    let c = Clip {
                        pattern: pid,
                        track: track as u16,
                        start,
                        length: len,
                        offset: 0,
                        muted: false,
                        selected: true,
                    };
                    clips.push(c);
                    let idx = clips.len() - 1;
                    let kind = if self.playlist.tool == Tool::Paint {
                        Kind::Paint { track: track as u16, next: start + len }
                    } else {
                        Kind::Move { anchor_tick: tick, anchor_track: track, originals: vec![(idx, c)], lead: idx }
                    };
                    let label = if self.playlist.tool == Tool::Paint { "Paint clips" } else { "Add clip" };
                    self.playlist.drag = Some(Drag { kind, before, changed: true, label });
                    self.needs_sync = true;
                }
            }
            Tool::Delete => {
                self.pl_begin(Kind::Erase, "Delete clips");
                self.pl_erase(v, p);
            }
            Tool::Mute => {
                if let Some(i) = hit {
                    self.checkpoint("Mute clip");
                    let c = &mut self.project.playlist.clips[i];
                    c.muted = !c.muted;
                }
            }
            Tool::Slice => {
                if let Some(i) = hit {
                    let at = snap_round(tick.max(0.0) as Tick, snap);
                    let before = self.project.clone();
                    if self.project.playlist.slice(i, at) {
                        self.checkpoint_from(&before, "Slice clip");
                    }
                }
            }
            Tool::Zoom => self.pl_zoom(v, p.x, 1.5),
        }
    }

    fn pl_zoom(&mut self, v: View, x: f32, factor: f32) {
        let t = v.tick(x);
        let pl = &mut self.playlist;
        pl.px_per_tick = (pl.px_per_tick * factor).clamp(0.002, 0.5);
        pl.scroll_tick = (t - (x - v.grid.left()) / pl.px_per_tick).max(0.0);
    }

    fn pl_erase(&mut self, v: View, p: Pos2) {
        if let Some(i) = self.pl_hit(v, p) {
            self.project.playlist.clips.remove(i);
            if let Some(d) = &mut self.playlist.drag {
                d.changed = true;
            }
            self.needs_sync = true;
        }
    }

    fn pl_update(&mut self, v: View, p: Pos2, delta: egui::Vec2, mods: egui::Modifiers) {
        let Some(mut drag) = self.playlist.drag.take() else { return };
        let snap = if mods.alt { 0 } else { self.pl_snap() };
        let tracks = self.project.playlist.tracks.len() as i32;
        match &mut drag.kind {
            Kind::Pan => {
                self.playlist.scroll_tick = (self.playlist.scroll_tick - delta.x / self.playlist.px_per_tick).max(0.0);
                self.playlist.scroll_track = (self.playlist.scroll_track - delta.y / self.playlist.track_h)
                    .clamp(0.0, (tracks - 1).max(0) as f32);
            }
            Kind::Move { anchor_tick, anchor_track, originals, lead } => {
                let lo = originals.iter().find(|(i, _)| i == lead).map(|o| o.1).unwrap_or(originals[0].1);
                let raw = lo.start as f32 + (v.tick(p.x) - *anchor_tick);
                let new_start = if snap > 0 { snap_round(raw.max(0.0) as Tick, snap) } else { raw.max(0.0) as Tick };
                let min_start = originals.iter().map(|o| o.1.start).min().unwrap_or(0);
                let dt = (new_start - lo.start).max(-min_start);
                let (tmin, tmax) = originals
                    .iter()
                    .fold((i32::MAX, 0), |(a, b), o| (a.min(o.1.track as i32), b.max(o.1.track as i32)));
                let dk = (v.track(p.y) - *anchor_track).clamp(-tmin, tracks - 1 - tmax);
                let clips = &mut self.project.playlist.clips;
                for (i, o) in originals.iter() {
                    if let Some(c) = clips.get_mut(*i) {
                        let (s, t) = (o.start + dt, (o.track as i32 + dk) as u16);
                        if c.start != s || c.track != t {
                            drag.changed = true;
                            self.needs_sync = true;
                        }
                        c.start = s;
                        c.track = t;
                    }
                }
            }
            Kind::Resize { anchor_tick, originals, lead } => {
                let lo = originals.iter().find(|(i, _)| i == lead).map(|o| o.1).unwrap_or(originals[0].1);
                let raw_end = lo.end() as f32 + (v.tick(p.x) - *anchor_tick);
                let new_end = if snap > 0 { snap_round(raw_end as Tick, snap) } else { raw_end as Tick };
                let dl = new_end - lo.end();
                let min_len = self.project.time_signature.ticks_per_beat() / 4;
                let clips = &mut self.project.playlist.clips;
                for (i, o) in originals.iter() {
                    if let Some(c) = clips.get_mut(*i) {
                        let l = (o.length + dl).max(min_len);
                        if c.length != l {
                            drag.changed = true;
                            self.needs_sync = true;
                        }
                        c.length = l;
                    }
                }
            }
            Kind::Erase => {
                if let Some(i) = self.pl_hit(v, p) {
                    self.project.playlist.clips.remove(i);
                    drag.changed = true;
                    self.needs_sync = true;
                }
            }
            Kind::Paint { track, next } => {
                let t = v.tick(p.x).max(0.0) as Tick;
                let pid = self.project.current_pattern;
                let len = self.project.current().length_ticks(self.project.time_signature).max(1);
                while t >= *next {
                    self.project.playlist.clips.push(Clip {
                        pattern: pid,
                        track: *track,
                        start: *next,
                        length: len,
                        offset: 0,
                        muted: false,
                        selected: true,
                    });
                    *next += len;
                    drag.changed = true;
                    self.needs_sync = true;
                }
            }
            Kind::Select { .. } => {}
        }
        self.playlist.drag = Some(drag);
    }

    fn pl_end(&mut self, v: View, pos: Option<Pos2>) {
        let Some(drag) = self.playlist.drag.take() else { return };
        if let (Kind::Select { start, additive }, Some(p)) = (&drag.kind, pos) {
            let r = Rect::from_two_pos(*start, p);
            let additive = *additive;
            for c in self.project.playlist.clips.iter_mut() {
                let hit = v.clip_rect(c).intersects(r);
                c.selected = if additive { c.selected || hit } else { hit };
            }
            return;
        }
        if drag.changed {
            self.project.playlist.clips.sort_by_key(|c| (c.track, c.start));
            self.checkpoint_from(&drag.before, drag.label);
        }
    }

    fn draw_playlist_grid(&self, painter: &egui::Painter, v: View) {
        let g = v.grid;
        painter.rect_filled(g, 0.0, theme::GRID_BG);
        let n_tracks = self.project.playlist.tracks.len();
        let first = v.top.floor().max(0.0) as usize;
        let last = ((v.top + g.height() / v.th).ceil() as usize).min(n_tracks);
        for t in first..last {
            let y = v.y(t as f32);
            if t % 2 == 1 {
                painter.rect_filled(
                    Rect::from_min_max(pos2(g.left(), y), pos2(g.right(), y + v.th)),
                    0.0,
                    theme::GRID_BLACK_KEY,
                );
            }
            painter.hline(g.x_range(), y + v.th, Stroke::new(1.0, theme::GRID_LINE));
        }
        let ts = self.project.time_signature;
        let (beat, bar) = (ts.ticks_per_beat(), ts.ticks_per_bar());
        let mut t = snap_floor(v.tick(g.left()).max(0.0) as Tick, beat);
        let t1 = v.tick(g.right()) as Tick;
        let show_beats = beat as f32 * v.px > 8.0;
        while t <= t1 {
            let x = v.x(t as f32);
            if t % bar == 0 {
                painter.vline(x, g.y_range(), Stroke::new(1.0, theme::GRID_BAR));
            } else if show_beats {
                painter.vline(x, g.y_range(), Stroke::new(1.0, theme::GRID_LINE));
            }
            t += beat;
        }
        // Song end marker.
        let end_x = v.x(self.project.playlist.song_length(ts) as f32);
        if g.x_range().contains(end_x) {
            painter.vline(end_x, g.y_range(), Stroke::new(1.0, theme::WARN.gamma_multiply(0.6)));
        }
        let font = FontId::proportional(11.0);
        for c in &self.project.playlist.clips {
            let r = v.clip_rect(c);
            if !r.intersects(g) {
                continue;
            }
            let base = pattern_color(&self.project, c.pattern);
            let track_muted = self.project.playlist.tracks.get(c.track as usize).is_some_and(|t| t.muted);
            let fill = if c.muted || track_muted {
                Color32::from_gray(80)
            } else if c.selected {
                base.lerp_to_gamma(Color32::WHITE, 0.35)
            } else {
                base.gamma_multiply(0.75)
            };
            painter.rect_filled(r, 3.0, fill);
            painter.rect_stroke(
                r,
                3.0,
                Stroke::new(1.0, if c.selected { Color32::WHITE } else { Color32::from_black_alpha(140) }),
                StrokeKind::Inside,
            );
            let name = self.project.pattern(c.pattern).map(|p| p.name.as_str()).unwrap_or("?");
            let title = Rect::from_min_max(r.min, pos2(r.max.x, r.min.y + 13.0));
            painter.rect_filled(title, 3.0, Color32::from_black_alpha(60));
            painter.with_clip_rect(r).text(
                title.left_center() + vec2(4.0, 0.0),
                Align2::LEFT_CENTER,
                name,
                font.clone(),
                Color32::BLACK,
            );
            // Mini preview of the pattern's notes.
            if let Some(p) = self.project.pattern(c.pattern) {
                let body = Rect::from_min_max(pos2(r.min.x, title.max.y + 1.0), pos2(r.max.x, r.max.y - 2.0));
                let notes: Vec<_> = p.channels.iter().flat_map(|pc| pc.notes.iter()).collect();
                if let (Some(lo), Some(hi)) = (notes.iter().map(|n| n.key).min(), notes.iter().map(|n| n.key).max()) {
                    let span = (hi - lo).max(1) as f32;
                    let clip_painter = painter.with_clip_rect(body);
                    for n in notes {
                        let rel = n.start - c.offset;
                        if rel < 0 || rel >= c.length {
                            continue;
                        }
                        let x0 = v.x((c.start + rel) as f32);
                        let x1 = v.x((c.start + rel + n.length).min(c.end()) as f32).max(x0 + 1.5);
                        let y = body.bottom() - 2.0 - (n.key - lo) as f32 / span * (body.height() - 4.0);
                        clip_painter.rect_filled(
                            Rect::from_min_max(pos2(x0, y - 1.0), pos2(x1, y + 1.0)),
                            0.0,
                            Color32::from_black_alpha(170),
                        );
                    }
                }
            }
        }
        if self.project.song_mode {
            let x = v.x(self.shared.position_tick() as f32);
            if g.x_range().contains(x) {
                painter.vline(x, g.y_range(), Stroke::new(1.5, theme::PLAYHEAD));
            }
        }
    }

    fn draw_track_headers(&self, _ui: &Ui, painter: &egui::Painter, v: View, r: Rect) {
        painter.rect_filled(r, 0.0, theme::PANEL_DARK);
        let n_tracks = self.project.playlist.tracks.len();
        let first = v.top.floor().max(0.0) as usize;
        let last = ((v.top + r.height() / v.th).ceil() as usize).min(n_tracks);
        for t in first..last {
            let y = v.y(t as f32);
            let tr = &self.project.playlist.tracks[t];
            painter.hline(r.x_range(), y + v.th, Stroke::new(1.0, theme::GRID_LINE));
            painter.circle_filled(
                pos2(r.left() + 11.0, y + v.th / 2.0),
                4.5,
                if tr.muted { theme::LED_OFF } else { theme::LED_ON },
            );
            painter.text(
                pos2(r.left() + 22.0, y + v.th / 2.0),
                Align2::LEFT_CENTER,
                &tr.name,
                FontId::proportional(12.0),
                if tr.muted { theme::TEXT_DIM } else { theme::TEXT },
            );
        }
    }

    fn draw_playlist_ruler(&self, painter: &egui::Painter, v: View, r: Rect) {
        painter.rect_filled(r, 0.0, theme::PANEL_DARK);
        let bar = self.project.time_signature.ticks_per_bar();
        let mut t = snap_floor(v.tick(r.left()).max(0.0) as Tick, bar);
        let t1 = v.tick(r.right()) as Tick;
        let skip = ((40.0 / (bar as f32 * v.px)).ceil() as i64).max(1);
        while t <= t1 {
            let n = t / bar + 1;
            if (n - 1) % skip == 0 {
                let x = v.x(t as f32);
                painter.vline(x, r.y_range(), Stroke::new(1.0, theme::GRID_BAR));
                painter.text(
                    pos2(x + 3.0, r.center().y),
                    Align2::LEFT_CENTER,
                    n.to_string(),
                    FontId::proportional(11.0),
                    theme::TEXT,
                );
            }
            t += bar;
        }
        if self.project.song_mode {
            let x = v.x(self.shared.position_tick() as f32);
            if r.x_range().contains(x) {
                painter.add(egui::Shape::convex_polygon(
                    vec![pos2(x - 5.0, r.top()), pos2(x + 5.0, r.top()), pos2(x, r.bottom())],
                    theme::PLAYHEAD,
                    Stroke::NONE,
                ));
            }
        }
    }

    // ----- commands --------------------------------------------------------

    fn pl_selected_or_all(&self) -> Vec<usize> {
        let c = &self.project.playlist.clips;
        let sel: Vec<usize> = (0..c.len()).filter(|&i| c[i].selected).collect();
        if sel.is_empty() { (0..c.len()).collect() } else { sel }
    }

    pub fn playlist_command(&mut self, cmd: Command) -> bool {
        use Command::*;
        let bar = self.project.time_signature.ticks_per_bar();
        let step = {
            let s = self.pl_snap();
            if s > 0 { s } else { self.project.time_signature.ticks_per_beat() }
        };
        match cmd {
            ToolDraw | ToolPaint | ToolDelete | ToolMute | ToolSlice | ToolSelect | ToolZoom => {
                self.playlist.tool = match cmd {
                    ToolDraw => Tool::Draw,
                    ToolPaint => Tool::Paint,
                    ToolDelete => Tool::Delete,
                    ToolMute => Tool::Mute,
                    ToolSlice => Tool::Slice,
                    ToolSelect => Tool::Select,
                    _ => Tool::Zoom,
                }
            }
            SelectAll | Deselect | InvertSelection => {
                for c in &mut self.project.playlist.clips {
                    c.selected = match cmd {
                        SelectAll => true,
                        Deselect => false,
                        _ => !c.selected,
                    };
                }
            }
            DeleteSelection => {
                if self.project.playlist.clips.iter().any(|c| c.selected) {
                    self.checkpoint("Delete clips");
                    self.project.playlist.clips.retain(|c| !c.selected);
                }
            }
            Copy | Cut => {
                let idx = self.pl_selected_or_all();
                self.playlist.clipboard = idx.iter().map(|&i| self.project.playlist.clips[i]).collect();
                if cmd == Cut && !idx.is_empty() {
                    self.checkpoint("Cut clips");
                    let mut k = 0;
                    self.project.playlist.clips.retain(|_| {
                        let keep = !idx.contains(&k);
                        k += 1;
                        keep
                    });
                }
                self.set_status(format!("{} clips copied.", self.playlist.clipboard.len()));
            }
            Paste => {
                if self.playlist.clipboard.is_empty() {
                    self.set_status("Clipboard is empty.");
                    return true;
                }
                // Paste at the song position, keeping relative placement.
                let at = snap_floor(self.shared.position_tick().max(0.0) as Tick, bar);
                let first = self.playlist.clipboard.iter().map(|c| c.start).min().unwrap_or(0);
                self.checkpoint("Paste clips");
                let clip = self.playlist.clipboard.clone();
                let clips = &mut self.project.playlist.clips;
                clips.iter_mut().for_each(|c| c.selected = false);
                clips.extend(clip.into_iter().map(|mut c| {
                    c.start = c.start - first + at;
                    c.selected = true;
                    c
                }));
            }
            DuplicateRight => {
                let idx = self.pl_selected_or_all();
                if idx.is_empty() {
                    return true;
                }
                self.checkpoint("Duplicate clips");
                let clips = &mut self.project.playlist.clips;
                let start = idx.iter().map(|&i| clips[i].start).min().unwrap_or(0);
                let end = idx.iter().map(|&i| clips[i].end()).max().unwrap_or(0);
                let span = (((end - snap_floor(start, bar)) + bar - 1) / bar).max(1) * bar;
                let copies: Vec<Clip> = idx.iter().map(|&i| clips[i]).collect();
                clips.iter_mut().for_each(|c| c.selected = false);
                clips.extend(copies.into_iter().map(|mut c| {
                    c.start += span;
                    c.selected = true;
                    c
                }));
            }
            MoveSelectionLeft | MoveSelectionRight | MoveSelectionUp | MoveSelectionDown => {
                let idx = self.pl_selected_or_all();
                if idx.is_empty() {
                    return true;
                }
                self.checkpoint("Move clips");
                let max_track = self.project.playlist.tracks.len() as i32 - 1;
                let clips = &mut self.project.playlist.clips;
                let (dt, dk) = match cmd {
                    MoveSelectionLeft => (-step, 0),
                    MoveSelectionRight => (step, 0),
                    MoveSelectionUp => (0, -1),
                    _ => (0, 1),
                };
                let min_start = idx.iter().map(|&i| clips[i].start).min().unwrap_or(0);
                let (tmin, tmax) = idx
                    .iter()
                    .fold((i32::MAX, 0), |(a, b), &i| (a.min(clips[i].track as i32), b.max(clips[i].track as i32)));
                let dt = dt.max(-min_start);
                let dk = dk.clamp(-tmin, max_track - tmax);
                for &i in &idx {
                    clips[i].start += dt;
                    clips[i].track = (clips[i].track as i32 + dk) as u16;
                }
            }
            MuteSelection | UnmuteSelection => {
                self.checkpoint("Mute clips");
                let m = cmd == MuteSelection;
                for i in self.pl_selected_or_all() {
                    self.project.playlist.clips[i].muted = m;
                }
            }
            PlQuickQuantizeStarts => {
                self.checkpoint("Quantize clip starts");
                for i in self.pl_selected_or_all() {
                    let c = &mut self.project.playlist.clips[i];
                    c.start = snap_round(c.start, step).max(0);
                }
            }
            PlSliceAtCursor => {
                let at = self.shared.position_tick() as Tick;
                let before = self.project.clone();
                let hits: Vec<usize> = (0..self.project.playlist.clips.len())
                    .filter(|&i| {
                        let c = self.project.playlist.clips[i];
                        at > c.start && at < c.end()
                    })
                    .collect();
                let mut any = false;
                for i in hits {
                    any |= self.project.playlist.slice(i, at);
                }
                if any {
                    self.checkpoint_from(&before, "Slice at cursor");
                }
            }
            PlSelectBySource => {
                let pats: Vec<PatternId> =
                    self.project.playlist.clips.iter().filter(|c| c.selected).map(|c| c.pattern).collect();
                let target = if pats.is_empty() { vec![self.project.current_pattern] } else { pats };
                for c in &mut self.project.playlist.clips {
                    c.selected = target.contains(&c.pattern);
                }
            }
            ZoomIn => self.playlist.px_per_tick = (self.playlist.px_per_tick * 1.25).min(0.5),
            ZoomOut => self.playlist.px_per_tick = (self.playlist.px_per_tick / 1.25).max(0.002),
            ZoomAll => {
                let len = self.project.playlist.song_length(self.project.time_signature) as f32;
                self.playlist.px_per_tick = (self.playlist.last_w / len).clamp(0.002, 0.5);
                self.playlist.scroll_tick = 0.0;
            }
            CenterOnPlayhead => {
                let t = self.shared.position_tick() as f32;
                self.playlist.scroll_tick = (t - self.playlist.last_w / 2.0 / self.playlist.px_per_tick).max(0.0);
            }
            _ => return false,
        }
        self.needs_sync = true;
        true
    }
}
