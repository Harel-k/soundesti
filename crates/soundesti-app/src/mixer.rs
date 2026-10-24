//! Mixer: master + 125 insert strips with faders, pan, mute, solo and live meters.
//! Routing: every insert feeds the master (insert-to-insert sends arrive in Phase 4).

use egui::{RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use soundesti_commands::Command;

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
                    format!("← {}", routed.join(", "))
                })
                .color(theme::TEXT_DIM)
                .small(),
            );
            ui.label(RichText::new("Effect slots and sends: Phase 4").color(theme::TEXT_DIM).small());
        });
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
