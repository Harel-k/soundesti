//! Channel Rack: channels, step sequencer, per-channel volume/pan/mute/mixer routing.

use egui::{Color32, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2};
use soundesti_commands::Command;
use soundesti_project::{ChannelId, DEFAULT_NOTE_KEY, Note, Project};
use soundesti_timeline::{TICKS_PER_STEP, Tick};

use crate::app::{App, Dialog};
use crate::{theme, widgets};

const STEP_W: f32 = 17.0;
const STEP_H: f32 = 24.0;

/// Step-paint gesture: left-drag paints steps on, right-drag erases (FL convention).
pub struct StepPaint {
    pub on: bool,
    pub before: Project,
    pub changed: bool,
}

/// A channel shows step buttons only while all its notes are plain steps; otherwise
/// FL shows a mini piano-roll preview instead, and so do we.
fn is_step_only(notes: &[Note]) -> bool {
    notes.iter().all(|n| n.key == DEFAULT_NOTE_KEY && n.length == TICKS_PER_STEP && n.start % TICKS_PER_STEP == 0)
}

impl App {
    pub fn channel_rack_ui(&mut self, ui: &mut Ui) {
        let ts = self.project.time_signature;
        let steps = (self.project.current().length_ticks(ts) / TICKS_PER_STEP).max(16);
        let ids: Vec<ChannelId> = self.project.channels.iter().map(|c| c.id).collect();
        let mut open_piano: Option<ChannelId> = None;
        let mut dropped: Option<(Option<ChannelId>, std::path::PathBuf)> = None;
        let dragging_sample = egui::DragAndDrop::has_payload_of_type::<crate::browser::DraggedSample>(ui.ctx());

        egui::ScrollArea::both().auto_shrink([false, true]).show(ui, |ui| {
            for (row, id) in ids.iter().copied().enumerate() {
                let row_resp = ui.horizontal(|ui| {
                    ui.set_height(STEP_H + 2.0);
                    self.channel_controls(ui, id, row);
                    let notes = self.project.current().notes(id).to_vec();
                    if is_step_only(&notes) {
                        self.step_buttons(ui, id, steps);
                    } else if self.mini_roll(ui, id, &notes, steps).clicked() {
                        open_piano = Some(id);
                    }
                    let m = self.session.slot_of(id).map(|s| self.channel_meters[s]).unwrap_or_default();
                    let (r, _) = ui.allocate_exact_size(vec2(6.0, STEP_H), Sense::hover());
                    widgets::meter(ui, r, m.left.max(m.right), m.left.max(m.right));
                    let selected = self.selected_channels.contains(&id);
                    let (r, resp) = ui.allocate_exact_size(vec2(8.0, STEP_H), Sense::click());
                    ui.painter().rect_filled(r.shrink(1.0), 2.0, if selected { theme::LED_ON } else { theme::LED_OFF });
                    if resp.on_hover_text("Select (Ctrl+click to add)").clicked() {
                        let additive = ui.input(|i| i.modifiers.ctrl);
                        self.select_channel(id, additive);
                    }
                });
                // Dropping a Browser sample on a row replaces that channel's sound.
                let r = ui.interact(row_resp.response.rect, ui.scope_id().with(("rack_drop", id.0)), Sense::hover());
                if dragging_sample && r.contains_pointer() {
                    ui.painter().rect_stroke(
                        r.rect,
                        2.0,
                        egui::Stroke::new(1.5, theme::ACCENT),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(p) = r.dnd_release_payload::<crate::browser::DraggedSample>() {
                    dropped = Some((Some(id), p.0.clone()));
                }
            }
            // The empty area below the rows adds a new channel.
            let (rest, _) = ui.allocate_exact_size(
                vec2(ui.available_width(), ui.available_height().clamp(STEP_H * 2.0, 300.0)),
                Sense::hover(),
            );
            let r = ui.interact(rest, ui.scope_id().with("rack_drop_new"), Sense::hover());
            if dragging_sample && r.contains_pointer() {
                ui.painter().rect_stroke(rest, 2.0, egui::Stroke::new(1.5, theme::ACCENT), egui::StrokeKind::Inside);
                ui.painter().text(
                    rest.center(),
                    egui::Align2::CENTER_CENTER,
                    "Drop to add a new channel",
                    egui::FontId::proportional(13.0),
                    theme::ACCENT,
                );
            }
            if let Some(p) = r.dnd_release_payload::<crate::browser::DraggedSample>() {
                dropped = Some((None, p.0.clone()));
            }
        });
        match dropped {
            Some((Some(id), path)) => self.set_channel_sample(id, &path),
            Some((None, path)) => self.add_sample_channel(&path),
            None => {}
        }
        // End any step-paint gesture once the buttons are released.
        if ui.input(|i| !i.pointer.any_down())
            && let Some(p) = self.rack_paint.take()
            && p.changed
        {
            self.checkpoint_from(&p.before, "Edit steps");
        }

        ui.horizontal(|ui| {
            ui.menu_button(RichText::new(" + Add ").strong(), |ui| {
                self.instrument_menu(ui);
            })
            .response
            .on_hover_text("Add a channel");
            ui.label(
                RichText::new(format!("{} channels · pattern {} steps", ids.len(), steps))
                    .color(theme::TEXT_DIM)
                    .small(),
            );
        });

        if let Some(id) = open_piano {
            self.select_channel(id, false);
            self.win.piano_roll = true;
            self.focus = soundesti_shortcuts::Context::PianoRoll;
        }
    }

    fn channel_controls(&mut self, ui: &mut Ui, id: ChannelId, row: usize) {
        let Some(ch) = self.project.channel(id).cloned() else { return };
        let soloed = self.session.solo.contains(&id);
        let led = widgets::led(ui, !ch.muted, "Mute (click) · Solo (right-click / Ctrl+click)");
        if led.secondary_clicked() || (led.clicked() && ui.input(|i| i.modifiers.ctrl)) {
            if soloed {
                self.session.solo.retain(|c| *c != id);
            } else {
                self.session.solo = vec![id];
            }
            self.needs_sync = true;
        } else if led.clicked() {
            self.checkpoint("Mute channel");
            if let Some(c) = self.project.channel_mut(id) {
                c.muted = !c.muted;
            }
        }
        let mut pan = ch.pan;
        let r = widgets::knob(ui, &mut pan, -1.0, 1.0, 0.0, 20.0, theme::ACCENT, "Panning");
        self.channel_knob(r, id, "Channel pan", |c| c.pan = pan);
        let mut vol = ch.volume;
        let tip = format!("Volume {:.0}%", vol * 100.0);
        let r = widgets::knob(ui, &mut vol, 0.0, 1.25, 0.78, 20.0, theme::ACCENT, &tip);
        self.channel_knob(r, id, "Channel volume", |c| c.volume = vol);
        let mut track = ch.mixer_track as i32;
        let tr = ui
            .add(
                egui::DragValue::new(&mut track)
                    .range(0..=125)
                    .speed(0.1)
                    .custom_formatter(|v, _| if v == 0.0 { "M".into() } else { format!("{v:.0}") }),
            )
            .on_hover_text("Target mixer track (M = master)");
        if tr.drag_started() || tr.gained_focus() {
            self.checkpoint("Route channel");
        }
        if tr.changed()
            && let Some(c) = self.project.channel_mut(id)
        {
            c.mixer_track = track as u16;
            self.needs_sync = true;
        }

        let sel = self.selected_channel == Some(id);
        let name = egui::Button::new(RichText::new(&ch.name).color(Color32::BLACK).strong())
            .fill(theme::rgb(ch.color).gamma_multiply(if sel { 1.0 } else { 0.75 }))
            .min_size(vec2(110.0, STEP_H))
            .stroke(if sel { Stroke::new(1.5, Color32::WHITE) } else { Stroke::NONE });
        let resp = ui.add(name).on_hover_text("Click: open instrument · Right-click: channel menu");
        if resp.clicked() {
            self.select_channel(id, ui.input(|i| i.modifiers.ctrl));
            if !self.win.instruments.insert(id) {
                self.win.instruments.remove(&id);
            }
        }
        resp.context_menu(|ui| {
            if ui.button("Piano roll").clicked() {
                self.select_channel(id, false);
                self.win.piano_roll = true;
                ui.close();
            }
            if ui.button("Rename…").clicked() {
                self.dialog = Some(Dialog::RenameChannel { id, text: ch.name.clone() });
                ui.close();
            }
            if ui.button("Clone").clicked() {
                self.select_channel(id, false);
                self.rack_command(Command::ChannelClone, None);
                ui.close();
            }
            if ui.button("Route to free mixer track (Ctrl+L)").clicked() {
                self.select_channel(id, false);
                self.rack_command(Command::ChannelRouteToFreeTracks, None);
                ui.close();
            }
            if ui.button("Move up").clicked() {
                self.select_channel(id, false);
                self.rack_command(Command::ChannelMoveUp, None);
                ui.close();
            }
            if ui.button("Move down").clicked() {
                self.select_channel(id, false);
                self.rack_command(Command::ChannelMoveDown, None);
                ui.close();
            }
            ui.separator();
            if ui.button(RichText::new("Delete (Alt+Del)").color(theme::DANGER)).clicked() {
                self.select_channel(id, false);
                self.rack_command(Command::ChannelDelete, None);
                ui.close();
            }
        });
        let _ = row;
    }

    fn channel_knob(
        &mut self,
        r: widgets::ControlResponse,
        id: ChannelId,
        label: &str,
        apply: impl FnOnce(&mut soundesti_project::Channel),
    ) {
        if r.started {
            self.checkpoint(label);
        }
        if r.changed
            && let Some(c) = self.project.channel_mut(id)
        {
            apply(c);
            self.needs_sync = true;
        }
    }

    fn step_buttons(&mut self, ui: &mut Ui, id: ChannelId, steps: i64) {
        let playing_step = if self.shared.playing.load(std::sync::atomic::Ordering::Relaxed) {
            Some((self.shared.position_tick() as Tick) / TICKS_PER_STEP)
        } else {
            None
        };
        let (pos, down1, down2, p1, p2) = ui.input(|i| {
            (
                i.pointer.interact_pos(),
                i.pointer.primary_down(),
                i.pointer.secondary_down(),
                i.pointer.primary_pressed(),
                i.pointer.secondary_pressed(),
            )
        });
        for s in 0..steps {
            let (rect, resp) = ui.allocate_exact_size(vec2(STEP_W, STEP_H), Sense::click_and_drag());
            let on = self.project.current().step_is_on(id, s);
            let group = (s / 4) % 2 == 0;
            let base = if group { theme::STEP_A } else { theme::STEP_B };
            let r = rect.shrink2(vec2(1.5, 2.0));
            let p = ui.painter();
            p.rect_filled(r, 2.0, if on { theme::STEP_ON } else { base });
            if playing_step == Some(s) {
                p.rect_stroke(r, 2.0, Stroke::new(1.5, theme::PLAYHEAD), StrokeKind::Outside);
            } else if resp.hovered() {
                p.rect_stroke(r, 2.0, Stroke::new(1.0, theme::TEXT_DIM), StrokeKind::Outside);
            }

            let over = pos.is_some_and(|q| rect.contains(q));
            if over && (p1 || p2) && self.rack_paint.is_none() {
                // Left press toggles; the drag then paints that state. Right press erases.
                let target = if p1 { !on } else { false };
                self.rack_paint =
                    Some(crate::channel_rack::StepPaint { on: target, before: self.project.clone(), changed: false });
                if target {
                    self.preview_on(id, DEFAULT_NOTE_KEY, soundesti_project::DEFAULT_VELOCITY);
                }
            }
            if over
                && (down1 || down2)
                && let Some(paint) = &mut self.rack_paint
                && self.project.current().step_is_on(id, s) != paint.on
            {
                paint.changed = true;
                self.project.current_mut().toggle_step(id, s);
                self.needs_sync = true;
            }
        }
        if !(down1 || down2) {
            self.preview_off();
        }
    }

    fn mini_roll(&self, ui: &mut Ui, id: ChannelId, notes: &[Note], steps: i64) -> egui::Response {
        let w = STEP_W * steps as f32;
        let (rect, resp) = ui.allocate_exact_size(vec2(w, STEP_H), Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, 2.0, theme::PANEL_DARK);
        let (lo, hi) = notes.iter().fold((127u8, 0u8), |(a, b), n| (a.min(n.key), b.max(n.key)));
        let span = (hi.saturating_sub(lo) as f32).max(1.0);
        let total = (steps * TICKS_PER_STEP) as f32;
        let color = self.project.channel(id).map(|c| theme::rgb(c.color)).unwrap_or(theme::ACCENT);
        for n in notes {
            let x0 = rect.left() + n.start as f32 / total * w;
            let x1 = rect.left() + n.end() as f32 / total * w;
            let y = rect.bottom() - 3.0 - (n.key - lo) as f32 / span * (rect.height() - 6.0);
            p.rect_filled(Rect::from_min_max(pos2(x0, y - 1.5), pos2(x1.max(x0 + 2.0), y + 1.5)), 1.0, color);
        }
        resp.on_hover_text("This channel has piano-roll notes. Click to open the Piano roll.")
    }

    pub fn rack_command(&mut self, cmd: Command, arg: Option<i32>) -> bool {
        use Command::*;
        let selected: Vec<ChannelId> =
            self.project.channels.iter().map(|c| c.id).filter(|id| self.selected_channels.contains(id)).collect();
        match cmd {
            ChannelToggleMute => {
                let n = arg.unwrap_or(1) as usize;
                if let Some(id) = self.project.channels.get(n.saturating_sub(1)).map(|c| c.id) {
                    self.checkpoint("Mute channel");
                    if let Some(c) = self.project.channel_mut(id) {
                        c.muted = !c.muted;
                    }
                }
            }
            ChannelToggleSolo => {
                let n = arg.unwrap_or(1) as usize;
                if let Some(id) = self.project.channels.get(n.saturating_sub(1)).map(|c| c.id) {
                    if self.session.solo.contains(&id) {
                        self.session.solo.clear();
                    } else {
                        self.session.solo = vec![id];
                    }
                }
            }
            ChannelSelectNext => self.step_channel(1, false),
            ChannelSelectPrevious => self.step_channel(-1, false),
            ChannelExtendSelectionDown | ChannelExtendSelectionUp => {
                let d = if cmd == ChannelExtendSelectionDown { 1 } else { -1 };
                let prev = self.selected_channels.clone();
                self.step_channel(d, false);
                self.selected_channels.extend(prev);
            }
            ChannelClone => {
                if selected.is_empty() {
                    return true;
                }
                self.checkpoint("Clone channels");
                let mut last = None;
                for id in selected {
                    last = self.project.clone_channel(id);
                }
                if let Some(id) = last {
                    self.select_channel(id, false);
                }
            }
            ChannelDelete => {
                if selected.is_empty() {
                    return true;
                }
                self.checkpoint("Delete channels");
                for id in &selected {
                    self.project.remove_channel(*id);
                }
                self.selected_channels.clear();
                self.fix_selection();
                self.set_status(format!("Deleted {} channel(s). Ctrl+Z to undo.", selected.len()));
            }
            ChannelMoveUp | ChannelMoveDown => {
                let Some(id) = self.selected_channel else { return true };
                let Some(i) = self.project.channels.iter().position(|c| c.id == id) else { return true };
                let j = if cmd == ChannelMoveUp {
                    i.checked_sub(1)
                } else {
                    (i + 1 < self.project.channels.len()).then_some(i + 1)
                };
                if let Some(j) = j {
                    self.checkpoint("Move channel");
                    self.project.channels.swap(i, j);
                }
            }
            ChannelCopySteps | ChannelCutSteps => {
                let Some(id) = self.selected_channel else { return true };
                self.step_clipboard = Some(self.project.current().notes(id).to_vec());
                if cmd == ChannelCutSteps {
                    self.checkpoint("Cut steps");
                    self.project.current_mut().notes_mut(id).clear();
                }
                self.set_status("Channel steps copied.");
            }
            ChannelPasteSteps => {
                let (Some(id), Some(clip)) = (self.selected_channel, self.step_clipboard.clone()) else {
                    self.set_status("Nothing to paste.");
                    return true;
                };
                self.checkpoint("Paste steps");
                *self.project.current_mut().notes_mut(id) = clip;
            }
            ChannelRouteToFreeTracks => {
                if selected.is_empty() {
                    return true;
                }
                self.checkpoint("Route to free mixer tracks");
                let mut used: Vec<u16> = self.project.channels.iter().map(|c| c.mixer_track).collect();
                for id in selected {
                    let free = (1..=125u16).find(|t| !used.contains(t));
                    if let (Some(t), Some(ch)) = (free, self.project.channel_mut(id)) {
                        ch.mixer_track = t;
                        let name = ch.name.clone();
                        used.push(t);
                        self.project.mixer[t as usize].name = name;
                        self.selected_track = t as usize;
                    }
                }
            }
            ChannelShiftStepsLeft | ChannelShiftStepsRight => {
                let Some(id) = self.selected_channel else { return true };
                let len = self.project.current().length_ticks(self.project.time_signature);
                let d = if cmd == ChannelShiftStepsRight { TICKS_PER_STEP } else { -TICKS_PER_STEP };
                self.checkpoint("Shift steps");
                for n in self.project.current_mut().notes_mut(id).iter_mut() {
                    n.start = (n.start + d).rem_euclid(len);
                }
            }
            _ => return false,
        }
        self.needs_sync = true;
        true
    }
}
