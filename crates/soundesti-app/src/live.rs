//! Live input and recording: MIDI keyboards (played on the selected channel, like FL),
//! note recording into the current pattern (R), and audio recording from the default
//! input device into a new Sampler channel.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use egui::{RichText, Ui};
use soundesti_audio::Command as EngineCmd;
use soundesti_project::{ChannelId, Note};
use soundesti_timeline::{PPQ, Tick};

use crate::app::App;
use crate::theme;

pub const MIDI_PORT_KEY: &str = "midi_input_port";

/// A note being held (from MIDI or the typing keyboard).
struct Held {
    slot: usize,
    channel: ChannelId,
    /// Song position when pressed, if recording.
    start: Option<f64>,
    velocity: f32,
}

struct AudioRecorder {
    _stream: cpal::Stream,
    rx: rtrb::Consumer<f32>,
    samples: Vec<f32>,
    channels: u16,
    sample_rate: u32,
    start_tick: f64,
    started: Instant,
}

pub struct LiveInput {
    midi: Option<midir::MidiInputConnection<()>>,
    midi_rx: Option<Receiver<[u8; 3]>>,
    pub midi_port: Option<String>,
    midi_error: Option<String>,
    /// Record arm (FL's R / the toolbar record button).
    pub armed: bool,
    /// Also record audio from the input device while armed and playing.
    pub record_audio: bool,
    held: HashMap<u8, Held>,
    /// One undo step per recording pass.
    pass_checkpointed: bool,
    audio: Option<AudioRecorder>,
    was_playing: bool,
}

impl LiveInput {
    pub fn new(saved_port: Option<String>) -> Self {
        Self {
            midi: None,
            midi_rx: None,
            midi_port: saved_port,
            midi_error: None,
            armed: false,
            record_audio: false,
            held: HashMap::new(),
            pass_checkpointed: false,
            audio: None,
            was_playing: false,
        }
    }
}

/// Names of the MIDI input ports on this PC.
pub fn midi_ports() -> Vec<String> {
    let Ok(m) = midir::MidiInput::new("Soundesti") else { return Vec::new() };
    m.ports().iter().filter_map(|p| m.port_name(p).ok()).collect()
}

impl App {
    /// Opens the MIDI input port called `name` (None disconnects).
    pub fn connect_midi(&mut self, ctx: &egui::Context, name: Option<String>) {
        self.live.midi = None;
        self.live.midi_rx = None;
        self.live.midi_error = None;
        self.live.midi_port = name.clone();
        let Some(name) = name else { return };
        let result = (|| -> Result<_, String> {
            let input = midir::MidiInput::new("Soundesti").map_err(|e| e.to_string())?;
            let port = input
                .ports()
                .into_iter()
                .find(|p| input.port_name(p).ok().as_deref() == Some(&name))
                .ok_or("device not found (unplugged?)")?;
            let (tx, rx) = channel();
            let wake = ctx.clone();
            let conn = input
                .connect(
                    &port,
                    "soundesti-in",
                    move |_ts, msg, _| {
                        if msg.len() >= 3 {
                            let _ = tx.send([msg[0], msg[1], msg[2]]);
                            wake.request_repaint();
                        }
                    },
                    (),
                )
                .map_err(|e| e.to_string())?;
            Ok((conn, rx))
        })();
        match result {
            Ok((conn, rx)) => {
                self.live.midi = Some(conn);
                self.live.midi_rx = Some(rx);
                self.set_status(format!("MIDI input: {name}"));
            }
            Err(e) => {
                self.live.midi_error = Some(e.clone());
                self.set_status(format!("Could not open MIDI input {name}: {e}"));
            }
        }
    }

    /// A key went down (MIDI or typing keyboard) on the selected channel.
    pub fn live_note_on(&mut self, key: u8, velocity: f32) {
        let Some(ch) = self.selected_channel else { return };
        let Some(slot) = self.session.slot_of(ch) else { return };
        if let Some(old) = self.live.held.remove(&key) {
            self.session.send(EngineCmd::NoteOff { slot: old.slot, key });
        }
        self.session.send(EngineCmd::NoteOn { slot, key, velocity });
        let playing = self.shared.playing.load(std::sync::atomic::Ordering::Relaxed);
        let start = (self.live.armed && playing).then(|| self.shared.position_tick());
        self.live.held.insert(key, Held { slot, channel: ch, start, velocity });
    }

    pub fn live_note_off(&mut self, key: u8) {
        let Some(h) = self.live.held.remove(&key) else { return };
        self.session.send(EngineCmd::NoteOff { slot: h.slot, key });
        if let Some(start) = h.start {
            self.record_note(h.channel, key, h.velocity, start, self.shared.position_tick());
        }
    }

    /// Writes a played note into the current pattern (wrapping at the pattern loop).
    fn record_note(&mut self, ch: ChannelId, key: u8, velocity: f32, start: f64, end: f64) {
        let ts = self.project.time_signature;
        let len = self.project.current().length_ticks(ts).max(ts.ticks_per_bar());
        let (s, length) = recorded_span(start, end, len);
        if !self.live.pass_checkpointed {
            self.checkpoint("Record notes");
            self.live.pass_checkpointed = true;
        }
        let mut n = Note::new(s, length, key);
        n.velocity = velocity;
        let notes = self.project.current_mut().notes_mut(ch);
        notes.push(n);
        notes.sort_by_key(|n| (n.start, n.key));
        self.needs_sync = true;
    }

    /// Per-frame: MIDI messages, transport start/stop for audio recording.
    pub fn tick_live(&mut self) {
        let msgs: Vec<[u8; 3]> = self.live.midi_rx.as_ref().map(|rx| rx.try_iter().collect()).unwrap_or_default();
        for m in msgs {
            match m[0] & 0xF0 {
                0x90 if m[2] > 0 => self.live_note_on(m[1].min(127), m[2] as f32 / 127.0),
                0x80 | 0x90 => self.live_note_off(m[1].min(127)),
                _ => {}
            }
        }
        let playing = self.shared.playing.load(std::sync::atomic::Ordering::Relaxed);
        if playing && !self.live.was_playing {
            self.live.pass_checkpointed = false;
            if self.live.armed && self.live.record_audio {
                self.start_audio_recording();
            }
        }
        if !playing && self.live.was_playing {
            self.finish_audio_recording();
        }
        self.live.was_playing = playing;
        if let Some(rec) = &mut self.live.audio {
            while let Ok(s) = rec.rx.pop() {
                rec.samples.push(s);
            }
        }
    }

    pub fn toggle_record(&mut self) {
        self.live.armed = !self.live.armed;
        if !self.live.armed {
            self.finish_audio_recording();
        }
        self.set_status(if self.live.armed {
            if self.live.record_audio {
                "Recording armed: notes and audio are recorded while playing."
            } else {
                "Recording armed: notes you play are recorded while playing."
            }
        } else {
            "Recording off."
        });
    }

    fn start_audio_recording(&mut self) {
        let result = (|| -> Result<AudioRecorder, String> {
            let host = cpal::default_host();
            let device = host.default_input_device().ok_or("no input device (microphone or line in) found")?;
            let config = device.default_input_config().map_err(|e| e.to_string())?;
            let channels = config.channels();
            let sample_rate = config.sample_rate();
            let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(sample_rate as usize * channels as usize * 2);
            let err = |e: cpal::Error| log::warn!("input stream error: {e}");
            let stream = match config.sample_format() {
                cpal::SampleFormat::F32 => device.build_input_stream(
                    config.into(),
                    move |data: &[f32], _: &cpal::InputCallbackInfo| {
                        for &s in data {
                            let _ = tx.push(s);
                        }
                    },
                    err,
                    None,
                ),
                cpal::SampleFormat::I16 => device.build_input_stream(
                    config.into(),
                    move |data: &[i16], _: &cpal::InputCallbackInfo| {
                        for &s in data {
                            let _ = tx.push(s as f32 / 32_768.0);
                        }
                    },
                    err,
                    None,
                ),
                f => return Err(format!("unsupported input format {f:?}")),
            }
            .map_err(|e| e.to_string())?;
            stream.play().map_err(|e| e.to_string())?;
            Ok(AudioRecorder {
                _stream: stream,
                rx,
                samples: Vec::new(),
                channels,
                sample_rate,
                start_tick: self.shared.position_tick(),
                started: Instant::now(),
            })
        })();
        match result {
            Ok(r) => {
                self.live.audio = Some(r);
                self.set_status("Recording audio…");
            }
            Err(e) => self.set_status(format!("Could not record audio: {e}")),
        }
    }

    /// Saves the take as a WAV and puts it on a new Sampler channel at the take's position.
    fn finish_audio_recording(&mut self) {
        let Some(mut rec) = self.live.audio.take() else { return };
        while let Ok(s) = rec.rx.pop() {
            rec.samples.push(s);
        }
        drop(rec._stream);
        let frames = rec.samples.len() / rec.channels.max(1) as usize;
        if frames < rec.sample_rate as usize / 10 {
            self.set_status("Recording was too short to keep.");
            return;
        }
        let dir = recordings_dir();
        let _ = std::fs::create_dir_all(&dir);
        let stamp = chrono_stamp();
        let path = dir.join(format!("Recording {stamp}.wav"));
        let spec = hound::WavSpec {
            channels: rec.channels,
            sample_rate: rec.sample_rate,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };
        let written = hound::WavWriter::create(&path, spec).and_then(|mut w| {
            for s in &rec.samples {
                w.write_sample(*s)?;
            }
            w.finalize()
        });
        if let Err(e) = written {
            self.set_status(format!("Could not save the recording: {e}"));
            return;
        }
        // A one-shot sampler note at the take's start plays it back in place.
        self.checkpoint("Audio recording");
        let name = format!("Recording {stamp}");
        let id = self.project.add_channel(&name, soundesti_synthesis::sampler::TYPE_ID);
        let seconds = frames as f64 / rec.sample_rate as f64;
        let ticks = (seconds * self.project.bpm / 60.0 * PPQ as f64).round() as Tick;
        let ts = self.project.time_signature;
        let len = self.project.current().length_ticks(ts) as f64;
        if let Some(c) = self.project.channel_mut(id) {
            c.instrument.sample = Some(path.display().to_string());
        }
        let start = rec.start_tick.rem_euclid(len.max(1.0)).round() as Tick;
        self.project.current_mut().notes_mut(id).push(Note::new(start, ticks.max(1), 60));
        self.select_channel(id, false);
        self.needs_sync = true;
        self.set_status(format!(
            "Saved {:.1} s recording to {} (new channel \"{name}\").",
            rec.started.elapsed().as_secs_f32().min(seconds as f32),
            path.display()
        ));
    }

    pub fn midi_settings_ui(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.label(RichText::new("MIDI input").strong());
        let ports = midi_ports();
        if ports.is_empty() {
            ui.label(
                RichText::new("No MIDI input devices found. Plug in a MIDI keyboard and reopen this window.")
                    .color(theme::TEXT_DIM),
            );
        }
        let current = self.live.midi_port.clone();
        let connected = self.live.midi.is_some();
        for p in &ports {
            let on = connected && current.as_deref() == Some(p.as_str());
            if ui
                .selectable_label(on, p)
                .on_hover_text("Click to use this device; notes play the selected channel")
                .clicked()
            {
                self.connect_midi(&ctx, if on { None } else { Some(p.clone()) });
            }
        }
        if let Some(e) = &self.live.midi_error {
            ui.label(RichText::new(e).color(theme::WARN));
        }
        if !connected
            && let Some(p) = &current
            && !ports.contains(p)
        {
            ui.label(RichText::new(format!("Saved device \"{p}\" is not connected.")).color(theme::TEXT_DIM));
        }
        ui.separator();
        ui.label(RichText::new("Recording").strong());
        ui.checkbox(&mut self.live.record_audio, "Record audio from the input device too")
            .on_hover_text("While recording is armed (R) and the song plays, the default microphone / line input is recorded into a new Sampler channel.");
        ui.label(
            RichText::new("Press R (or the record button) to arm recording, then play: notes you play on the MIDI or typing keyboard are written into the current pattern on the selected channel.")
                .small()
                .color(theme::TEXT_DIM),
        );
    }
}

/// Pattern position and length of a note held from song position `start` to `end`,
/// in a pattern that loops every `pattern_len` ticks.
fn recorded_span(start: f64, end: f64, pattern_len: Tick) -> (Tick, Tick) {
    let len = pattern_len.max(1) as f64;
    let mut length = end - start;
    if length <= 0.0 {
        // The loop wrapped while the key was held.
        length += len;
    }
    (start.rem_euclid(len).round() as Tick, (length.round() as Tick).max(PPQ / 32))
}

fn recordings_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default()
        .join("Documents")
        .join("Soundesti")
        .join("Recordings")
}

/// Local-time-free timestamp for file names: seconds since 2020 in a readable form.
fn chrono_stamp() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Civil date from Unix time (UTC), Howard Hinnant's algorithm.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {:02}.{:02}.{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recorded_notes_land_in_the_pattern_loop() {
        let bar = 3840;
        assert_eq!(recorded_span(960.0, 1920.0, bar), (960, 960));
        // Song position past the pattern end folds back into the loop.
        assert_eq!(recorded_span(bar as f64 + 480.0, bar as f64 + 720.0, bar), (480, 240));
        // Held across the loop point: the position jumped back.
        assert_eq!(recorded_span(3600.0, 240.0, bar), (3600, 480));
        // A tap still gets a minimum length.
        assert_eq!(recorded_span(100.0, 100.4, bar).1, PPQ / 32);
        assert!(chrono_stamp().len() == 19);
    }
}
