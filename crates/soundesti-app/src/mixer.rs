//! Mixer: master + 125 insert strips with faders, pan, mute, solo and live meters, plus the
//! selected track's 10 insert effect slots on the right (FL Studio's layout).
//! Routing: every insert feeds the master (insert-to-insert sends are not implemented yet).

use egui::{Layout, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use soundesti_commands::Command;
use soundesti_project::FX_SLOTS;

use crate::app::{App, Dialog};
use crate::{theme, widgets};

const STRIP_W: f32 = 54.0;

impl App {
    pub fn mixer_ui(&mut self, ui: &mut Ui) {
        // Show the master, then every insert up to the last one that is used or named, plus a few spares.
        let last_used = self
            .project
            .channels
            .iter()
            .map(|c| c.mixer_track as usize)
            .chain(std::iter::once(self.selected_track))
            .max()
            .unwrap_or(0);
        let shown = (last_used + 8).clamp(16, self.project.mixer.len() - 1);
        let inserts_w = 220.0;
        let strips_w = (ui.available_width() - inserts_w - 12.0).max(120.0);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                vec2(strips_w, ui.available_height()),
                Layout::top_down(egui::Align::Min),
                |ui| {
                    self.mixer_strips(ui, shown);
                },
            );
            ui.separator();
            ui.allocate_ui_with_layout(
                vec2(inserts_w, ui.available_height()),
                Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(inserts_w);
                    self.insert_slots_ui(ui);
                },
            );
        });
    }

    fn mixer_strips(&mut self, ui: &mut Ui, shown: usize) {
        egui::ScrollArea::horizontal().auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal(|ui| {
                for i in 0..=shown {
                    self.mixer_strip(ui, i);
                    if i == 0 {
                        ui.separator();
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            let t = self.selected_track;
            let routed: Vec<String> =
                self.project.channels.iter().filter(|c| c.mixer_track as usize == t).map(|c| c.name.clone()).collect();
            let name = &self.project.mixer[t].name;
            ui.label(RichText::new(format!("Selected: {name}")).color(theme::ACCENT));
            ui.label(
                RichText::new(if routed.is_empty() {
                    "no channels routed".to_string()
                } else {
                    format!("from {}", routed.join(", "))
                })
                .color(theme::TEXT_DIM)
                .small(),
            );
        });
    }

    /// The selected track's insert chain: 10 slots, processed top to bottom before the fader.
    fn insert_slots_ui(&mut self, ui: &mut Ui) {
        let t = self.selected_track;
        let track = self.project.mixer[t].clone();
        ui.label(RichText::new(format!("{} - inserts", track.name)).strong().color(theme::ACCENT));
        for slot in 0..FX_SLOTS {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{}", slot + 1)).small().color(theme::TEXT_DIM));
                match track.effect(slot) {
                    Some(fx) => {
                        let name = match &fx.plugin {
                            Some(p) => p.name.clone(),
                            None => soundesti_effects::info(&fx.type_id)
                                .map(|i| i.name)
                                .unwrap_or("Unknown effect")
                                .to_string(),
                        };
                        let is_plugin = fx.plugin.is_some();
                        if widgets::led(ui, !fx.bypass, "Enable / bypass this effect").clicked() {
                            self.checkpoint("Bypass effect");
                            if let Some(e) = self.project.mixer[t].effect_mut(slot) {
                                e.bypass = !e.bypass;
                            }
                            self.needs_sync = true;
                        }
                        let text = RichText::new(name).color(if fx.bypass { theme::TEXT_DIM } else { theme::TEXT });
                        let r = ui
                            .add_sized(
                                [150.0, 20.0],
                                egui::Button::new(text).selected(self.win.effects.contains(&(t, slot))),
                            )
                            .on_hover_text("Click to open; right-click for options");
                        if r.clicked() && is_plugin {
                            self.open_plugin_editor(soundesti_session::PluginKey::Effect(t, slot));
                        } else if r.clicked() && !self.win.effects.remove(&(t, slot)) {
                            self.win.effects.insert((t, slot));
                        }
                        r.context_menu(|ui| {
                            ui.menu_button("Replace", |ui| self.effect_menu(ui, t, slot));
                            if is_plugin && ui.button("Parameters…").clicked() {
                                self.win.effects.insert((t, slot));
                                ui.close();
                            }
                            if slot > 0 && ui.button("Move up").clicked() {
                                self.move_effect(t, slot, slot - 1);
                                ui.close();
                            }
                            if slot + 1 < FX_SLOTS && ui.button("Move down").clicked() {
                                self.move_effect(t, slot, slot + 1);
                                ui.close();
                            }
                            if ui.button("Delete").clicked() {
                                self.checkpoint("Remove effect");
                                self.project.mixer[t].remove_effect(slot);
                                self.win.effects.remove(&(t, slot));
                                self.needs_sync = true;
                                ui.close();
                            }
                        });
                    }
                    None => {
                        ui.add_space(18.0);
                        ui.scope(|ui| {
                            ui.set_width(150.0);
                            ui.menu_button(RichText::new("(empty)").color(theme::TEXT_DIM), |ui| {
                                self.effect_menu(ui, t, slot)
                            })
                            .response
                            .on_hover_text("Choose an effect for this slot");
                        });
                    }
                }
            });
        }
        ui.label(RichText::new("Effects run top to bottom, before the fader.").small().color(theme::TEXT_DIM));
    }

    /// Catalog of native effects grouped by category; choosing one fills `slot`.
    pub fn effect_menu(&mut self, ui: &mut Ui, track: usize, slot: usize) {
        let mut last = "";
        for info in soundesti_effects::CATALOG {
            if info.category != last {
                if !last.is_empty() {
                    ui.separator();
                }
                last = info.category;
                ui.label(RichText::new(info.category).small().color(theme::TEXT_DIM));
            }
            if ui.button(info.name).on_hover_text(info.description).clicked() {
                self.set_effect(track, slot, info.type_id);
                ui.close();
            }
        }
        self.plugin_effect_menu(ui, track, slot);
    }

    pub fn set_effect(&mut self, track: usize, slot: usize, type_id: &str) {
        if track >= self.project.mixer.len() || slot >= FX_SLOTS || soundesti_effects::info(type_id).is_none() {
            return;
        }
        self.checkpoint("Add effect");
        self.project.mixer[track].set_effect(slot, type_id);
        self.win.effects.insert((track, slot));
        self.needs_sync = true;
    }

    fn move_effect(&mut self, track: usize, from: usize, to: usize) {
        self.checkpoint("Move effect");
        // Moving recreates plugins in their new slot; keep their current settings.
        self.session.capture_plugin_states(&mut self.project);
        self.project.mixer[track].move_effect(from, to);
        let (a, b) = (self.win.effects.remove(&(track, from)), self.win.effects.remove(&(track, to)));
        if a {
            self.win.effects.insert((track, to));
        }
        if b {
            self.win.effects.insert((track, from));
        }
        self.needs_sync = true;
    }

    /// Body of a floating effect window (knobs for every parameter).
    pub fn effect_ui(&mut self, ui: &mut Ui, track: usize, slot: usize) {
        let Some(fx) = self.project.mixer.get(track).and_then(|t| t.effect(slot)).cloned() else { return };
        if fx.plugin.is_some() {
            self.plugin_params_ui(ui, soundesti_session::PluginKey::Effect(track, slot));
            return;
        }
        let params = soundesti_effects::params_for(&fx.type_id);
        ui.horizontal(|ui| {
            if widgets::led(ui, !fx.bypass, "Enable / bypass").clicked() {
                self.checkpoint("Bypass effect");
                if let Some(e) = self.project.mixer[track].effect_mut(slot) {
                    e.bypass = !e.bypass;
                }
                self.needs_sync = true;
            }
            ui.label(RichText::new(if fx.bypass { "Bypassed" } else { "Active" }).small().color(theme::TEXT_DIM));
            if ui.small_button("Reset").on_hover_text("Set every parameter back to its default").clicked() {
                self.checkpoint("Reset effect");
                if let Some(e) = self.project.mixer[track].effect_mut(slot) {
                    e.params.clear();
                }
                self.needs_sync = true;
            }
        });
        if let Some(p) = self.preset_bar_for(ui, format!("fx{track}.{slot}"), &fx.type_id, &fx.params) {
            self.checkpoint("Load preset");
            if let Some(e) = self.project.mixer[track].effect_mut(slot) {
                e.params = p;
            }
            self.needs_sync = true;
        }
        let edit = crate::params_ui::param_grid(ui, ("fx", track, slot), params, &fx.params, theme::ACCENT);
        if edit.started {
            self.checkpoint("Effect parameter");
        }
        if let Some((key, v)) = edit.changed
            && let Some(e) = self.project.mixer[track].effect_mut(slot)
        {
            e.params.insert(key.to_string(), v);
            self.needs_sync = true;
        }
    }

    fn mixer_strip(&mut self, ui: &mut Ui, i: usize) {
        let selected = self.selected_track == i;
        let frame = egui::Frame::new()
            .fill(if selected { theme::PANEL } else { theme::PANEL_DARK })
            .stroke(if selected { Stroke::new(1.0, theme::ACCENT) } else { Stroke::new(1.0, theme::GRID_LINE) })
            .corner_radius(3)
            .inner_margin(3);
        let resp = frame
            .show(ui, |ui| {
                ui.set_width(STRIP_W);
                ui.vertical_centered(|ui| {
                    let t = self.project.mixer[i].clone();
                    let label = if i == 0 { "Master".to_string() } else { i.to_string() };
                    ui.label(RichText::new(label).small().strong());
                    let name = egui::Label::new(RichText::new(truncate(&t.name, 8)).small().color(theme::TEXT_DIM))
                        .sense(Sense::click());
                    if ui.add(name).on_hover_text(format!("{} (double-click or F2 to rename)", t.name)).double_clicked()
                    {
                        self.dialog = Some(Dialog::RenameTrack { index: i, text: t.name.clone() });
                    }
                    ui.horizontal(|ui| {
                        let m = self.track_meters[i];
                        let (r, _) = ui.allocate_exact_size(vec2(12.0, 150.0), Sense::hover());
                        widgets::meter(ui, r, m.left, m.right);
                        let mut v = t.volume;
                        let db = if v > 0.0 { 20.0 * v.log10() } else { f32::NEG_INFINITY };
                        let r = widgets::fader(
                            ui,
                            &mut v,
                            1.25,
                            0.8,
                            vec2(26.0, 150.0),
                            theme::ACCENT,
                            &format!("Volume {db:+.1} dB"),
                        );
                        if r.started {
                            self.checkpoint("Mixer volume");
                        }
                        if r.changed {
                            self.project.mixer[i].volume = v;
                            self.needs_sync = true;
                        }
                    });
                    let mut pan = t.pan;
                    let r = widgets::knob(ui, &mut pan, -1.0, 1.0, 0.0, 22.0, theme::ACCENT, "Pan");
                    if r.started {
                        self.checkpoint("Mixer pan");
                    }
                    if r.changed {
                        self.project.mixer[i].pan = pan;
                        self.needs_sync = true;
                    }
                    ui.horizontal(|ui| {
                        if widgets::led(ui, !t.muted, "Mute").clicked() {
                            self.checkpoint("Mute track");
                            self.project.mixer[i].muted = !t.muted;
                            self.needs_sync = true;
                        }
                        if i > 0 {
                            let solo = self.mixer_solo == Some(i);
                            let s = ui
                                .add(egui::Button::new(RichText::new("S").small()).selected(solo))
                                .on_hover_text("Solo (S)");
                            if s.clicked() {
                                self.mixer_solo = if solo { None } else { Some(i) };
                                self.needs_sync = true;
                            }
                        }
                    });
                });
            })
            .response;
        // Any press inside the strip selects it, without stealing clicks from its controls.
        if resp.contains_pointer() && ui.input(|inp| inp.pointer.any_pressed()) {
            self.selected_track = i;
        }
        if selected {
            ui.painter().rect_stroke(resp.rect, 3.0, Stroke::new(1.5, theme::ACCENT), StrokeKind::Outside);
        }
    }

    pub fn mixer_command(&mut self, cmd: Command) -> bool {
        use Command::*;
        let t = self.selected_track;
        match cmd {
            MixerSolo => {
                if t > 0 {
                    self.mixer_solo = if self.mixer_solo == Some(t) { None } else { Some(t) };
                }
            }
            MixerLinkChannels => {
                if self.selected_channels.is_empty() {
                    self.set_status("Select channels in the Channel Rack first.");
                    return true;
                }
                self.checkpoint("Link channels to track");
                let ids = self.selected_channels.clone();
                for c in self.project.channels.iter_mut().filter(|c| ids.contains(&c.id)) {
                    c.mixer_track = t as u16;
                }
            }
            MixerLinkChannelsFrom => {
                self.checkpoint("Link channels from track");
                let ids = self.selected_channels.clone();
                for (next, c) in (t.max(1)..).zip(self.project.channels.iter_mut().filter(|c| ids.contains(&c.id))) {
                    c.mixer_track = next.min(125) as u16;
                }
            }
            MixerSelectLinkedChannels => {
                let ids: Vec<_> =
                    self.project.channels.iter().filter(|c| c.mixer_track as usize == t).map(|c| c.id).collect();
                self.selected_channels = ids.iter().copied().collect();
                self.selected_channel = ids.first().copied().or(self.selected_channel);
            }
            MixerRename => {
                let text = self.project.mixer[t].name.clone();
                self.dialog = Some(Dialog::RenameTrack { index: t, text });
            }
            MixerMoveLeft | MixerMoveRight => {
                let j = if cmd == MixerMoveLeft {
                    t.checked_sub(1).filter(|&j| j >= 1)
                } else {
                    (t + 1 < self.project.mixer.len()).then_some(t + 1)
                };
                if let (Some(j), true) = (j, t >= 1) {
                    self.checkpoint("Move mixer track");
                    self.project.mixer.swap(t, j);
                    for c in &mut self.project.channels {
                        let m = c.mixer_track as usize;
                        if m == t {
                            c.mixer_track = j as u16;
                        } else if m == j {
                            c.mixer_track = t as u16;
                        }
                    }
                    self.selected_track = j;
                }
            }
            _ => return false,
        }
        self.needs_sync = true;
        true
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).chain(std::iter::once('…')).collect() }
}
