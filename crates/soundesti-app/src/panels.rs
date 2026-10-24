//! Floating windows and modal dialogs.

use egui::{Align2, Id, RichText, Ui, vec2};
use soundesti_export::{BitDepth, ExportOptions};
use soundesti_plugin_api::ParamKind;
use soundesti_project::ChannelId;
use soundesti_shortcuts::Context;

use crate::app::{App, Dialog};
use crate::{theme, widgets};

impl App {
    pub fn windows(&mut self, ctx: &egui::Context) {
        let mut open = self.win.channel_rack;
        let r = egui::Window::new("Channel rack")
            .id(Id::unique("w_rack"))
            .open(&mut open)
            .default_pos([16.0, 96.0])
            .default_size([720.0, 220.0])
            .show(ctx, |ui| self.channel_rack_ui(ui));
        self.win.channel_rack = open;
        self.register_window(&r, Context::ChannelRack);

        let mut open = self.win.piano_roll;
        let title = format!(
            "Piano roll - {}",
            self.selected_channel.and_then(|c| self.project.channel(c)).map(|c| c.name.as_str()).unwrap_or("")
        );
        let r = egui::Window::new(title)
            .id(Id::unique("w_piano"))
            .open(&mut open)
            .default_pos([120.0, 340.0])
            .default_size([980.0, 470.0])
            .scroll(false)
            .resizable(true)
            .show(ctx, |ui| self.piano_roll_ui(ui));
        self.win.piano_roll = open;
        self.register_window(&r, Context::PianoRoll);

        let mut open = self.win.mixer;
        let r = egui::Window::new("Mixer")
            .id(Id::unique("w_mixer"))
            .open(&mut open)
            .default_pos([300.0, 120.0])
            .default_size([900.0, 300.0])
            .show(ctx, |ui| self.mixer_ui(ui));
        self.win.mixer = open;
        self.register_window(&r, Context::Mixer);

        let mut open = self.win.playlist;
        let r = egui::Window::new("Playlist - Arrangement").id(Id::unique("w_playlist")).open(&mut open).default_size([700.0, 300.0]).show(ctx, |ui| {
            not_yet(ui, "The Playlist (song arrangement) is Phase 3.", "Today you can build and loop patterns in the Channel Rack and Piano roll, mix them, save, and export WAV.");
        });
        self.win.playlist = open;
        self.register_window(&r, Context::Playlist);

        let mut open = self.win.browser;
        let r = egui::Window::new("Browser")
            .id(Id::unique("w_browser"))
            .open(&mut open)
            .default_pos([16.0, 400.0])
            .show(ctx, |ui| {
                not_yet(
                    ui,
                    "The sample/preset Browser is not implemented yet.",
                    "Native instruments are available from the Channel Rack ï¼‹ button or the Plugin picker (F8).",
                );
            });
        self.win.browser = open;
        self.register_window(&r, Context::Browser);

        let mut open = self.win.plugin_picker;
        egui::Window::new("Plugin picker")
            .id(Id::unique("w_picker"))
            .open(&mut open)
            .show(ctx, |ui| self.plugin_picker_ui(ui));
        self.win.plugin_picker = open;

        let mut open = self.win.song_info;
        egui::Window::new("Project info")
            .id(Id::unique("w_info"))
            .open(&mut open)
            .show(ctx, |ui| self.song_info_ui(ui));
        self.win.song_info = open;

        let mut open = self.win.midi_settings;
        egui::Window::new("MIDI settings").id(Id::unique("w_midi")).open(&mut open).show(ctx, |ui| {
            not_yet(
                ui,
                "MIDI device input/output is Phase 5.",
                "Use Typing keyboard to piano (Ctrl+T) to play instruments from the computer keyboard.",
            );
        });
        self.win.midi_settings = open;

        let mut open = self.win.audio_settings;
        egui::Window::new("Audio settings")
            .id(Id::unique("w_audio"))
            .open(&mut open)
            .show(ctx, |ui| self.audio_settings_ui(ui));
        self.win.audio_settings = open;

        let mut open = self.win.help;
        egui::Window::new("Keyboard shortcuts")
            .id(Id::unique("w_help"))
            .open(&mut open)
            .default_size([620.0, 480.0])
            .show(ctx, |ui| self.help_ui(ui));
        self.win.help = open;

        let mut open = self.win.quantize;
        egui::Window::new("Quantizer")
            .id(Id::unique("w_quant"))
            .open(&mut open)
            .show(ctx, |ui| self.quantize_window(ui));
        self.win.quantize = open;

        let mut open = self.win.ai_setup;
        egui::Window::new("AI assistant setup")
            .id(Id::unique("w_ai_setup"))
            .open(&mut open)
            .default_size([640.0, 480.0])
            .show(ctx, |ui| egui::ScrollArea::vertical().show(ui, |ui| self.ai_setup_ui(ui)));
        self.win.ai_setup = open;

        let mut open = self.win.export;
        egui::Window::new("Render to WAV")
            .id(Id::unique("w_export"))
            .open(&mut open)
            .collapsible(false)
            .show(ctx, |ui| self.export_ui(ui));
        self.win.export = open;

        if let Some(job) = &self.export_job {
            let frac = job.fraction();
            let mut cancel = false;
            egui::Window::new("Renderingâ€¦")
                .id(Id::unique("w_render"))
                .collapsible(false)
                .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(job.path.display().to_string());
                    ui.add(egui::ProgressBar::new(frac).show_percentage());
                    cancel = ui.button("Cancel").clicked();
                });
            if cancel {
                job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }

        let ids: Vec<ChannelId> = self.win.instruments.iter().copied().collect();
        for id in ids {
            let Some(name) = self.project.channel(id).map(|c| c.name.clone()) else { continue };
            let mut open = true;
            egui::Window::new(format!("{name} (Trident)"))
                .id(Id::unique(("w_inst", id.0)))
                .open(&mut open)
                .default_pos([420.0, 140.0])
                .resizable(false)
                .show(ctx, |ui| self.instrument_ui(ui, id));
            if !open {
                self.win.instruments.remove(&id);
            }
        }
    }

    fn plugin_picker_ui(&mut self, ui: &mut Ui) {
        ui.label(RichText::new("Instruments").strong());
        for (type_id, name) in soundesti_synthesis::INSTRUMENT_TYPES {
            ui.horizontal(|ui| {
                if ui.button(format!("ðŸŽ¹ {name}")).clicked() {
                    self.add_channel(type_id, name);
                }
                ui.label(RichText::new("3-oscillator subtractive synth").color(theme::TEXT_DIM).small());
            });
        }
        ui.separator();
        ui.label(RichText::new("Effects: none yet (Phase 4). CLAP/VST3 hosting: Phase 6.").color(theme::TEXT_DIM));
    }

    fn song_info_ui(&mut self, ui: &mut Ui) {
        egui::Grid::new("info").num_columns(2).show(ui, |ui| {
            let meta = self.project.meta.clone();
            let mut changed = meta.clone();
            ui.label("Title");
            let a = ui.text_edit_singleline(&mut changed.title);
            ui.end_row();
            ui.label("Author");
            let b = ui.text_edit_singleline(&mut changed.author);
            ui.end_row();
            ui.label("Genre");
            let c = ui.text_edit_singleline(&mut changed.genre);
            ui.end_row();
            ui.label("Comments");
            let d = ui.text_edit_multiline(&mut changed.comments);
            ui.end_row();
            if [&a, &b, &c, &d].iter().any(|r| r.gained_focus()) {
                self.checkpoint("Project info");
            }
            if changed != meta {
                self.project.meta = changed;
            }
        });
    }

    fn audio_settings_ui(&mut self, ui: &mut Ui) {
        let info = self.output.info.clone();
        ui.label(format!(
            "Running: {} Â· {} ({} Hz, {} ch, {})",
            info.host, info.device, info.sample_rate, info.channels, info.sample_format
        ));
        if let Some(l) = info.buffer_latency_ms() {
            ui.label(format!("Buffer latency: {l:.1} ms"));
        } else {
            ui.label("Buffer: driver default");
        }
        let s = &self.shared;
        ui.label(format!(
            "Callbacks: {} Â· overloaded: {} Â· dropped commands: {}",
            s.callbacks.load(std::sync::atomic::Ordering::Relaxed),
            s.overloads.load(std::sync::atomic::Ordering::Relaxed),
            s.dropped_commands.load(std::sync::atomic::Ordering::Relaxed)
        ));
        if let Some(e) = &self.audio_error {
            ui.colored_label(theme::WARN, e);
        }
        ui.separator();
        let mut set = self.audio_settings.clone();
        egui::ComboBox::from_label("Device")
            .selected_text(set.device.clone().unwrap_or_else(|| "System default".into()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut set.device, None, "System default");
                for d in &self.device_names {
                    ui.selectable_value(&mut set.device, Some(d.clone()), d);
                }
            });
        egui::ComboBox::from_label("Sample rate")
            .selected_text(set.sample_rate.map(|r| format!("{r} Hz")).unwrap_or_else(|| "Device default".into()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut set.sample_rate, None, "Device default");
                for r in [44_100, 48_000, 88_200, 96_000] {
                    ui.selectable_value(&mut set.sample_rate, Some(r), format!("{r} Hz"));
                }
            });
        egui::ComboBox::from_label("Buffer size")
            .selected_text(set.buffer_frames.map(|b| format!("{b} samples")).unwrap_or_else(|| "Driver default".into()))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut set.buffer_frames, None, "Driver default");
                for b in [64, 128, 256, 512, 1024, 2048] {
                    ui.selectable_value(&mut set.buffer_frames, Some(b), format!("{b} samples"));
                }
            });
        self.audio_settings = set;
        ui.horizontal(|ui| {
            if ui.button("Apply / restart audio").clicked() {
                self.restart_audio();
            }
            if ui.button("Refresh devices").clicked() {
                self.device_names = soundesti_audio_device::list_output_devices().into_iter().map(|d| d.name).collect();
            }
        });
        ui.label(
            RichText::new("WASAPI shared mode. ASIO / exclusive mode are not available yet.")
                .color(theme::TEXT_DIM)
                .small(),
        );
    }

    pub fn restart_audio(&mut self) {
        // Drop the old stream first so the device is free.
        let (null_out, null_handle) = soundesti_audio_device::start_null(48_000);
        self.output = null_out;
        drop(std::mem::replace(
            &mut self.session,
            soundesti_session::Session::new(null_handle, self.output.engine_config),
        ));
        let (output, handle, err) = soundesti_audio_device::start_or_fallback(&self.audio_settings);
        self.shared = handle.shared().clone();
        self.session = soundesti_session::Session::new(handle, output.engine_config);
        self.output = output;
        self.audio_error = err.map(|e| e.to_string());
        self.needs_sync = true;
        self.set_status(match &self.audio_error {
            None => format!("Audio running on {} @ {} Hz", self.output.info.device, self.output.info.sample_rate),
            Some(e) => format!("Audio fallback ({e}); now using {}", self.output.info.device),
        });
    }

    fn help_ui(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("ðŸ”");
            ui.add(
                egui::TextEdit::singleline(&mut self.help_query)
                    .hint_text("Search commands or keys, e.g. \"quantize\" or \"ctrl+b\"")
                    .desired_width(360.0),
            );
        });
        ui.label(RichText::new(format!("Profile: {} â€” transcribed from the official FL Studio manual. See docs/SHORTCUT_MATRIX.md for implementation status.", self.keymap.name)).color(theme::TEXT_DIM).small());
        ui.separator();
        let results: Vec<_> = self.keymap.search(&self.help_query).into_iter().cloned().collect();
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("keys").striped(true).num_columns(3).show(ui, |ui| {
                ui.label(RichText::new("Window").strong());
                ui.label(RichText::new("Keys").strong());
                ui.label(RichText::new("Command").strong());
                ui.end_row();
                for b in results {
                    ui.label(context_name(b.context));
                    ui.label(RichText::new(b.chord.to_string()).monospace().color(theme::ACCENT));
                    ui.label(b.command.label());
                    ui.end_row();
                }
            });
        });
    }

    fn export_ui(&mut self, ui: &mut Ui) {
        let o = &mut self.export_opts;
        egui::Grid::new("export").num_columns(2).show(ui, |ui| {
            ui.label("Content");
            ui.label(format!("Current pattern: {}", self.project.current().name));
            ui.end_row();
            ui.label("Loops");
            ui.add(egui::DragValue::new(&mut o.loops).range(1..=64));
            ui.end_row();
            ui.label("Tail");
            ui.add(egui::DragValue::new(&mut o.tail_seconds).range(0.0..=30.0).suffix(" s").speed(0.1));
            ui.end_row();
            ui.label("Sample rate");
            egui::ComboBox::from_id_salt("ex_sr").selected_text(format!("{} Hz", o.sample_rate)).show_ui(ui, |ui| {
                for r in [44_100, 48_000, 88_200, 96_000] {
                    ui.selectable_value(&mut o.sample_rate, r, format!("{r} Hz"));
                }
            });
            ui.end_row();
            ui.label("Bit depth");
            egui::ComboBox::from_id_salt("ex_bits").selected_text(bit_name(o.bit_depth)).show_ui(ui, |ui| {
                for b in [BitDepth::Int16, BitDepth::Int24, BitDepth::Float32] {
                    ui.selectable_value(&mut o.bit_depth, b, bit_name(b));
                }
            });
            ui.end_row();
        });
        ui.label(
            RichText::new("Song export (Playlist), FLAC, MP3 and stems arrive in later phases.")
                .color(theme::TEXT_DIM)
                .small(),
        );
        if ui.button("Renderâ€¦").clicked() {
            let opts: ExportOptions = self.export_opts;
            self.begin_export(opts);
        }
    }

    fn instrument_ui(&mut self, ui: &mut Ui, id: ChannelId) {
        let Some(ch) = self.project.channel(id).cloned() else { return };
        let params = soundesti_session::instrument_params(&ch.instrument.type_id);
        let color = theme::rgb(ch.color);
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
                                let cur = ch.instrument.params.get(p.key).copied().unwrap_or(p.default);
                                let mut v = cur;
                                let (started, changed) = ui
                                    .vertical(|ui| {
                                        ui.label(RichText::new(p.name).small());
                                        match p.kind {
                                            ParamKind::Stepped { labels } if !labels.is_empty() => {
                                                let mut idx = v.round() as usize;
                                                let mut ch = false;
                                                egui::ComboBox::from_id_salt((id.0, p.id))
                                                    .width(70.0)
                                                    .selected_text(labels[idx.min(labels.len() - 1)])
                                                    .show_ui(ui, |ui| {
                                                        for (i, l) in labels.iter().enumerate() {
                                                            ch |= ui.selectable_value(&mut idx, i, *l).changed();
                                                        }
                                                    });
                                                v = idx as f32;
                                                (ch, ch)
                                            }
                                            ParamKind::Stepped { .. } => {
                                                let r = ui.add(
                                                    egui::DragValue::new(&mut v)
                                                        .range(p.min..=p.max)
                                                        .speed(0.1)
                                                        .fixed_decimals(0)
                                                        .suffix(p.unit),
                                                );
                                                (r.drag_started() || r.gained_focus(), r.changed())
                                            }
                                            ParamKind::Continuous => {
                                                let log = p.unit == "Hz" && p.min > 0.0;
                                                let mut norm =
                                                    if log { (v / p.min).ln() / (p.max / p.min).ln() } else { v };
                                                let (lo, hi, def) = if log {
                                                    (0.0, 1.0, (p.default / p.min).ln() / (p.max / p.min).ln())
                                                } else {
                                                    (p.min, p.max, p.default)
                                                };
                                                let r = widgets::knob(
                                                    ui,
                                                    &mut norm,
                                                    lo,
                                                    hi,
                                                    def,
                                                    30.0,
                                                    color,
                                                    &format!("{} {}", p.name, fmt_value(v, p.unit)),
                                                );
                                                if r.changed {
                                                    v = if log { p.min * (p.max / p.min).powf(norm) } else { norm };
                                                }
                                                ui.label(
                                                    RichText::new(fmt_value(v, p.unit)).small().color(theme::TEXT_DIM),
                                                );
                                                (r.started, r.changed)
                                            }
                                        }
                                    })
                                    .inner;
                                if started {
                                    self.checkpoint("Instrument parameter");
                                }
                                if changed
                                    && v != cur
                                    && let Some(c) = self.project.channel_mut(id)
                                {
                                    c.instrument.params.insert(p.key.to_string(), v);
                                    self.needs_sync = true;
                                }
                            }
                        });
                    });
                });
            }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Preview:").small());
            for (label, key) in [("C4", 48u8), ("C5", 60), ("C6", 72)] {
                let r = ui.button(label);
                if r.is_pointer_button_down_on() {
                    if self.preview.map(|p| p.1) != Some(key) {
                        self.preview_on(id, key, 0.8);
                    }
                } else if r.drag_stopped() || r.clicked() {
                    self.preview_off();
                }
            }
        });
    }

    pub fn dialogs(&mut self, ctx: &egui::Context) {
        let Some(mut dialog) = self.dialog.take() else { return };
        let mut keep = true;
        let modal = |title: &str| {
            egui::Window::new(title.to_string())
                .collapsible(false)
                .resizable(false)
                .anchor(Align2::CENTER_CENTER, [0.0, -60.0])
        };
        match &mut dialog {
            Dialog::RenamePattern { id, text } => {
                let id = *id;
                modal("Rename pattern").show(ctx, |ui| {
                    if let Some(done) = rename_field(ui, text) {
                        if done && !text.trim().is_empty() {
                            self.checkpoint("Rename pattern");
                            if let Some(p) = self.project.pattern_mut(id) {
                                p.name = text.trim().to_string();
                            }
                        }
                        keep = false;
                    }
                });
            }
            Dialog::RenameChannel { id, text } => {
                let id = *id;
                modal("Rename channel").show(ctx, |ui| {
                    if let Some(done) = rename_field(ui, text) {
                        if done && !text.trim().is_empty() {
                            self.checkpoint("Rename channel");
                            if let Some(c) = self.project.channel_mut(id) {
                                c.name = text.trim().to_string();
                            }
                        }
                        keep = false;
                    }
                });
            }
            Dialog::RenameTrack { index, text } => {
                let i = *index;
                modal("Rename mixer track").show(ctx, |ui| {
                    if let Some(done) = rename_field(ui, text) {
                        if done && !text.trim().is_empty() {
                            self.checkpoint("Rename track");
                            self.project.mixer[i].name = text.trim().to_string();
                        }
                        keep = false;
                    }
                });
            }
            Dialog::ConfirmDiscard { then } => {
                let then = then.clone();
                modal("Unsaved changes").show(ctx, |ui| {
                    ui.label("Save changes to the current project?");
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            keep = false;
                            self.save(false);
                            if !self.dirty() {
                                self.after = Some(then.clone());
                            }
                        }
                        if ui.button("Don't save").clicked() {
                            keep = false;
                            self.after = Some(then.clone());
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            }
            Dialog::Message { title, text } => {
                modal(title).show(ctx, |ui| {
                    ui.label(text.as_str());
                    if ui.button("OK").clicked() {
                        keep = false;
                    }
                });
            }
        }
        if keep && self.dialog.is_none() {
            self.dialog = Some(dialog);
        }
    }
}

/// Text field + OK/Cancel. Returns Some(true) on OK/Enter, Some(false) on Cancel.
fn rename_field(ui: &mut Ui, text: &mut String) -> Option<bool> {
    let r = ui.add(egui::TextEdit::singleline(text).desired_width(240.0));
    if !r.has_focus() && !r.lost_focus() {
        r.request_focus();
    }
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.button("OK").clicked() || enter {
            out = Some(true);
        }
        if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            out = Some(false);
        }
    });
    out
}

fn not_yet(ui: &mut Ui, what: &str, meanwhile: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(format!("âš  {what}")).color(theme::WARN));
    ui.label(RichText::new(meanwhile).color(theme::TEXT_DIM));
    ui.add_space(8.0);
    ui.allocate_space(vec2(ui.available_width(), 0.0));
}

fn context_name(c: Context) -> &'static str {
    match c {
        Context::Global => "Global",
        Context::ChannelRack => "Channel rack",
        Context::PianoRoll => "Piano roll",
        Context::Playlist => "Playlist",
        Context::Mixer => "Mixer",
        Context::Browser => "Browser",
    }
}

fn bit_name(b: BitDepth) -> &'static str {
    match b {
        BitDepth::Int16 => "16-bit (dithered)",
        BitDepth::Int24 => "24-bit",
        BitDepth::Float32 => "32-bit float",
    }
}

fn fmt_value(v: f32, unit: &str) -> String {
    match unit {
        "Hz" if v >= 1000.0 => format!("{:.2} kHz", v / 1000.0),
        "Hz" => format!("{v:.0} Hz"),
        "s" if v < 1.0 => format!("{:.0} ms", v * 1000.0),
        "s" => format!("{v:.2} s"),
        "ct" => format!("{v:+.0} ct"),
        "Q" => format!("Q {v:.2}"),
        _ => format!("{:.0}%", v * 100.0),
    }
}
