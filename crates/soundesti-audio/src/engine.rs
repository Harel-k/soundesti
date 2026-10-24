use std::sync::Arc;
use std::time::Instant;

use rtrb::{Consumer, Producer, PushError, RingBuffer};
use soundesti_dsp::{Smoothed, balance_pan, enable_flush_denormals, sanitize};
use soundesti_plugin_api::{Effect, Instrument, NoteEvent, NoteId, TimedEvent};
use soundesti_timeline::Tick;

use crate::metronome::Metronome;
use crate::shared::Shared;
use crate::snapshot::PlaybackSnapshot;

/// Mixer track 0 is the master; 1..=125 are inserts.
pub const MASTER_TRACK: usize = 0;
pub const MIXER_TRACKS: usize = 126;
/// Insert effect slots per mixer track (FL Studio has 10).
pub const FX_SLOTS: usize = 10;

const COMMAND_QUEUE: usize = 4096;
const GARBAGE_QUEUE: usize = 1024;
const EVENTS_PER_CHANNEL: usize = 512;
const MAX_ACTIVE_NOTES: usize = 4096;
const PREVIEW_ID_BIT: u32 = 0x8000_0000;
const GAIN_RAMP_MS: f32 = 10.0;

#[derive(Clone, Copy, Debug)]
pub struct EngineConfig {
    pub sample_rate: f64,
    /// Largest block rendered at once; device buffers are split into blocks of this size.
    pub max_block: usize,
    /// Number of channel slots (Channel Rack rows).
    pub max_channels: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self { sample_rate: 48_000.0, max_block: 512, max_channels: 256 }
    }
}

/// Control-thread → audio-thread messages. Anything heap-allocated is built before sending.
pub enum Command {
    /// Start (or resume) playback.
    Play,
    /// Stop and return to the loop start.
    Stop,
    /// Stop without moving the play position.
    Pause,
    Seek(Tick),
    ReplaceSnapshot(Box<PlaybackSnapshot>),
    AddChannel {
        slot: usize,
        instrument: Box<dyn Instrument>,
    },
    RemoveChannel {
        slot: usize,
    },
    SetChannel {
        slot: usize,
        volume: f32,
        pan: f32,
        muted: bool,
        track: usize,
    },
    SetParam {
        slot: usize,
        id: u32,
        value: f32,
    },
    /// Hand a decoded sample to a sample-based instrument (None clears it).
    SetSample {
        slot: usize,
        sample: Option<std::sync::Arc<soundesti_plugin_api::SampleData>>,
    },
    /// Live/preview note (piano keys, step clicks, typing keyboard).
    NoteOn {
        slot: usize,
        key: u8,
        velocity: f32,
    },
    NoteOff {
        slot: usize,
        key: u8,
    },
    SetTrack {
        track: usize,
        volume: f32,
        pan: f32,
        muted: bool,
    },
    /// Put an effect in (or clear) an insert slot of a mixer track.
    SetEffect {
        track: usize,
        slot: usize,
        effect: Option<Box<dyn Effect>>,
    },
    SetEffectParam {
        track: usize,
        slot: usize,
        id: u32,
        value: f32,
    },
    SetEffectBypass {
        track: usize,
        slot: usize,
        bypass: bool,
    },
    SetMetronome(bool),
    /// Cut all sound immediately (Ctrl+H).
    Panic,
}

/// Objects handed back to the control thread to be freed there.
pub enum Garbage {
    Snapshot(Box<PlaybackSnapshot>),
    Instrument(Box<dyn Instrument>),
    Sample(std::sync::Arc<soundesti_plugin_api::SampleData>),
    Effect(Box<dyn Effect>),
}

/// Control-thread side of the engine.
pub struct EngineHandle {
    commands: Producer<Command>,
    garbage: Consumer<Garbage>,
    shared: Arc<Shared>,
}

impl EngineHandle {
    /// Queues a command. Returns false (and counts it) if the queue is full.
    pub fn send(&mut self, cmd: Command) -> bool {
        match self.commands.push(cmd) {
            Ok(()) => true,
            Err(_) => {
                self.shared.dropped_commands.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                false
            }
        }
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// Frees objects the audio thread has released. Call regularly (e.g. every UI frame).
    pub fn collect_garbage(&mut self) -> usize {
        let mut n = 0;
        while let Ok(g) = self.garbage.pop() {
            drop(g);
            n += 1;
        }
        n
    }
}

struct ChannelSlot {
    instrument: Box<dyn Instrument>,
    volume: Smoothed,
    pan: Smoothed,
    mute: Smoothed,
    track: usize,
}

struct Track {
    volume: Smoothed,
    pan: Smoothed,
    mute: Smoothed,
    left: Vec<f32>,
    right: Vec<f32>,
    used: bool,
    fx: [Option<Box<dyn Effect>>; FX_SLOTS],
    bypass: [bool; FX_SLOTS],
}

impl Track {
    fn has_fx(&self) -> bool {
        self.fx.iter().zip(self.bypass).any(|(f, b)| f.is_some() && !b)
    }

    /// Runs the insert chain in slot order (pre-fader, like FL Studio).
    fn run_fx(&mut self, n: usize) {
        for (fx, bypass) in self.fx.iter_mut().zip(self.bypass) {
            if let Some(fx) = fx
                && !bypass
            {
                fx.process(&mut self.left[..n], &mut self.right[..n]);
            }
        }
    }
}

#[derive(Clone, Copy)]
struct ActiveNote {
    channel: u16,
    id: NoteId,
    end: Tick,
}

/// Audio-thread side of the engine.
pub struct Engine {
    config: EngineConfig,
    commands: Consumer<Command>,
    garbage: Producer<Garbage>,
    shared: Arc<Shared>,
    snapshot: Box<PlaybackSnapshot>,
    channels: Vec<Option<ChannelSlot>>,
    events: Vec<Vec<TimedEvent>>,
    tracks: Vec<Track>,
    active: Vec<ActiveNote>,
    scratch_l: Vec<f32>,
    scratch_r: Vec<f32>,
    metronome: Metronome,
    metronome_on: bool,
    playing: bool,
    /// Absolute sample position under the current tempo map.
    play_sample: i64,
    next_id: NoteId,
}

impl Engine {
    pub fn new(config: EngineConfig) -> (Engine, EngineHandle) {
        let (cmd_tx, cmd_rx) = RingBuffer::new(COMMAND_QUEUE);
        let (gc_tx, gc_rx) = RingBuffer::new(GARBAGE_QUEUE);
        let sr = config.sample_rate as f32;
        let shared = Arc::new(Shared::new(config.sample_rate, MIXER_TRACKS, config.max_channels));
        let tracks = (0..MIXER_TRACKS)
            .map(|_| Track {
                volume: Smoothed::new(0.8, sr, GAIN_RAMP_MS),
                pan: Smoothed::new(0.0, sr, GAIN_RAMP_MS),
                mute: Smoothed::new(1.0, sr, GAIN_RAMP_MS),
                left: vec![0.0; config.max_block],
                right: vec![0.0; config.max_block],
                used: false,
                fx: Default::default(),
                bypass: [false; FX_SLOTS],
            })
            .collect();
        let engine = Engine {
            config,
            commands: cmd_rx,
            garbage: gc_tx,
            shared: shared.clone(),
            snapshot: Box::new(PlaybackSnapshot::empty(120.0)),
            channels: (0..config.max_channels).map(|_| None).collect(),
            events: (0..config.max_channels).map(|_| Vec::with_capacity(EVENTS_PER_CHANNEL)).collect(),
            tracks,
            active: Vec::with_capacity(MAX_ACTIVE_NOTES),
            scratch_l: vec![0.0; config.max_block],
            scratch_r: vec![0.0; config.max_block],
            metronome: Metronome::new(sr),
            metronome_on: false,
            playing: false,
            play_sample: 0,
            next_id: 1,
        };
        let handle = EngineHandle { commands: cmd_tx, garbage: gc_rx, shared };
        (engine, handle)
    }

    pub fn config(&self) -> EngineConfig {
        self.config
    }

    pub fn shared(&self) -> &Arc<Shared> {
        &self.shared
    }

    /// Renders `out` (interleaved, `out_channels` wide). Real-time safe.
    pub fn process(&mut self, out: &mut [f32], out_channels: usize) {
        enable_flush_denormals();
        let started = Instant::now();
        let oc = out_channels.max(1);
        let frames = out.len() / oc;
        let mut done = 0;
        while done < frames {
            let n = (frames - done).min(self.config.max_block);
            self.drain_commands();
            self.render_block(n);
            let master = &self.tracks[MASTER_TRACK];
            for i in 0..n {
                let frame = &mut out[(done + i) * oc..(done + i + 1) * oc];
                let (l, r) = (master.left[i], master.right[i]);
                if oc == 1 {
                    frame[0] = 0.5 * (l + r);
                } else {
                    frame[0] = l;
                    frame[1] = r;
                    frame[2..].fill(0.0);
                }
            }
            done += n;
        }
        let budget = frames as f64 / self.config.sample_rate;
        if budget > 0.0 {
            self.shared.report_load((started.elapsed().as_secs_f64() / budget) as f32);
        }
    }

    fn trash(&mut self, g: Garbage) {
        if let Err(PushError::Full(g)) = self.garbage.push(g) {
            // Never free on the audio thread; leaking is the lesser evil and is counted.
            std::mem::forget(g);
            self.shared.leaked_garbage.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }

    fn push_event(&mut self, slot: usize, offset: u32, event: NoteEvent) {
        if let Some(list) = self.events.get_mut(slot)
            && list.len() < list.capacity()
        {
            list.push(TimedEvent { offset, event });
        }
    }

    fn release_active(&mut self, offset: u32) {
        while let Some(a) = self.active.pop() {
            self.push_event(a.channel as usize, offset, NoteEvent::Off { id: a.id });
        }
    }

    fn current_tick(&self) -> f64 {
        self.snapshot.tempo.tick_at_sample(self.play_sample, self.config.sample_rate)
    }

    fn sample_at(&self, tick: Tick) -> i64 {
        self.snapshot.tempo.sample_at_tick(tick as f64, self.config.sample_rate)
    }

    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.commands.pop() {
            self.apply(cmd);
        }
    }

    fn apply(&mut self, cmd: Command) {
        let sr = self.config.sample_rate as f32;
        match cmd {
            Command::Play => {
                if !self.playing {
                    self.playing = true;
                    self.shared.playing.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            }
            Command::Stop => {
                self.playing = false;
                self.shared.playing.store(false, std::sync::atomic::Ordering::Relaxed);
                self.release_active(0);
                self.play_sample = self.sample_at(self.snapshot.loop_start);
            }
            Command::Pause => {
                self.playing = false;
                self.shared.playing.store(false, std::sync::atomic::Ordering::Relaxed);
                self.release_active(0);
            }
            Command::Seek(tick) => {
                self.release_active(0);
                let tick = tick.clamp(self.snapshot.loop_start, self.snapshot.loop_end - 1);
                self.play_sample = self.sample_at(tick);
            }
            Command::ReplaceSnapshot(new) => {
                // Keep the musical position when tempo or loop changes.
                let tick = self.current_tick();
                let old = std::mem::replace(&mut self.snapshot, new);
                let tick = if tick >= self.snapshot.loop_end as f64 || tick < self.snapshot.loop_start as f64 {
                    self.release_active(0);
                    self.snapshot.loop_start as f64
                } else {
                    tick
                };
                self.play_sample = self.snapshot.tempo.sample_at_tick(tick, self.config.sample_rate);
                self.trash(Garbage::Snapshot(old));
                let bpm = self.snapshot.tempo.bpm_at(tick as Tick) as f32;
                for fx in self.tracks.iter_mut().flat_map(|t| t.fx.iter_mut()).flatten() {
                    fx.set_tempo(bpm);
                }
            }
            Command::AddChannel { slot, instrument } => {
                if slot >= self.channels.len() {
                    self.trash(Garbage::Instrument(instrument));
                    return;
                }
                let new = ChannelSlot {
                    instrument,
                    volume: Smoothed::new(0.78, sr, GAIN_RAMP_MS),
                    pan: Smoothed::new(0.0, sr, GAIN_RAMP_MS),
                    mute: Smoothed::new(1.0, sr, GAIN_RAMP_MS),
                    track: MASTER_TRACK,
                };
                if let Some(old) = self.channels[slot].replace(new) {
                    self.trash(Garbage::Instrument(old.instrument));
                }
                self.events[slot].clear();
            }
            Command::RemoveChannel { slot } => {
                if let Some(old) = self.channels.get_mut(slot).and_then(Option::take) {
                    self.trash(Garbage::Instrument(old.instrument));
                }
                self.active.retain(|a| a.channel as usize != slot);
                if let Some(e) = self.events.get_mut(slot) {
                    e.clear();
                }
            }
            Command::SetChannel { slot, volume, pan, muted, track } => {
                if let Some(Some(ch)) = self.channels.get_mut(slot) {
                    ch.volume.set_target(volume.max(0.0));
                    ch.pan.set_target(pan.clamp(-1.0, 1.0));
                    ch.mute.set_target(if muted { 0.0 } else { 1.0 });
                    ch.track = track.min(MIXER_TRACKS - 1);
                }
            }
            Command::SetParam { slot, id, value } => {
                if let Some(Some(ch)) = self.channels.get_mut(slot) {
                    ch.instrument.set_param(id, value);
                }
            }
            Command::SetSample { slot, sample } => {
                let old = match self.channels.get_mut(slot) {
                    Some(Some(ch)) => ch.instrument.set_sample(sample),
                    _ => sample,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Sample(old));
                }
            }
            Command::NoteOn { slot, key, velocity } => {
                let id = preview_id(slot, key);
                self.push_event(slot, 0, NoteEvent::On { id, key, velocity });
            }
            Command::NoteOff { slot, key } => {
                self.push_event(slot, 0, NoteEvent::Off { id: preview_id(slot, key) });
            }
            Command::SetTrack { track, volume, pan, muted } => {
                if let Some(t) = self.tracks.get_mut(track) {
                    t.volume.set_target(volume.max(0.0));
                    t.pan.set_target(pan.clamp(-1.0, 1.0));
                    t.mute.set_target(if muted { 0.0 } else { 1.0 });
                }
            }
            Command::SetEffect { track, slot, effect } => {
                let bpm = self.snapshot.tempo.bpm_at(self.current_tick() as Tick) as f32;
                let old = match self.tracks.get_mut(track).and_then(|t| t.fx.get_mut(slot)) {
                    Some(place) => {
                        let mut effect = effect;
                        if let Some(fx) = effect.as_mut() {
                            fx.set_tempo(bpm);
                        }
                        std::mem::replace(place, effect)
                    }
                    None => effect,
                };
                if let Some(old) = old {
                    self.trash(Garbage::Effect(old));
                }
            }
            Command::SetEffectParam { track, slot, id, value } => {
                if let Some(Some(fx)) = self.tracks.get_mut(track).and_then(|t| t.fx.get_mut(slot)) {
                    fx.set_param(id, value);
                }
            }
            Command::SetEffectBypass { track, slot, bypass } => {
                if let Some(b) = self.tracks.get_mut(track).and_then(|t| t.bypass.get_mut(slot)) {
                    *b = bypass;
                }
            }
            Command::SetMetronome(on) => self.metronome_on = on,
            Command::Panic => {
                self.active.clear();
                for slot in 0..self.channels.len() {
                    if self.channels[slot].is_some() {
                        self.push_event(slot, 0, NoteEvent::AllSoundOff);
                    }
                }
            }
        }
    }

    fn alloc_id(&mut self) -> NoteId {
        self.next_id = self.next_id.wrapping_add(1) & !PREVIEW_ID_BIT;
        if self.next_id == 0 {
            self.next_id = 1;
        }
        self.next_id
    }

    /// Turns the snapshot's notes into timed events for the next `n` frames.
    fn schedule(&mut self, n: usize) {
        let sr = self.config.sample_rate;
        let mut done = 0usize;
        while done < n {
            let loop_start_s = self.sample_at(self.snapshot.loop_start);
            let loop_end_s = self.sample_at(self.snapshot.loop_end).max(loop_start_s + 1);
            if self.play_sample >= loop_end_s || self.play_sample < loop_start_s {
                self.release_active(done.min(n - 1) as u32);
                self.play_sample = loop_start_s;
            }
            let len = ((loop_end_s - self.play_sample) as usize).min(n - done);
            let s0 = self.play_sample;
            let s1 = s0 + len as i64;
            let t0 = self.snapshot.tempo.tick_at_sample(s0, sr);
            let t1 = self.snapshot.tempo.tick_at_sample(s1, sr);
            let rel = |engine: &Engine, tick: Tick| -> u32 {
                let s = engine.sample_at(tick) - s0;
                (s.clamp(0, len as i64 - 1) as usize + done) as u32
            };

            // Note-ons first so notes shorter than a block also get their note-off below.
            let notes = &self.snapshot.notes;
            let lo = notes.partition_point(|x| (x.start as f64) < t0);
            let hi = notes.partition_point(|x| (x.start as f64) < t1);
            for i in lo..hi {
                let note = self.snapshot.notes[i];
                let slot = note.channel as usize;
                if self.channels.get(slot).is_none_or(Option::is_none) {
                    continue;
                }
                let id = self.alloc_id();
                let offset = rel(self, note.start);
                self.push_event(slot, offset, NoteEvent::On { id, key: note.key, velocity: note.velocity });
                if self.active.len() < self.active.capacity() {
                    self.active.push(ActiveNote { channel: note.channel, id, end: note.end });
                }
            }

            let mut i = 0;
            while i < self.active.len() {
                let a = self.active[i];
                if (a.end as f64) < t1 {
                    let offset = rel(self, a.end);
                    self.push_event(a.channel as usize, offset, NoteEvent::Off { id: a.id });
                    self.active.swap_remove(i);
                } else {
                    i += 1;
                }
            }

            if self.metronome_on {
                let beat = self.snapshot.time_signature.ticks_per_beat();
                let bar = self.snapshot.time_signature.ticks_per_bar();
                let mut b = (t0 / beat as f64).ceil() as Tick * beat;
                while (b as f64) < t1 {
                    let offset = rel(self, b);
                    self.metronome.trigger(offset, b % bar == 0);
                    b += beat;
                }
            }

            self.play_sample = s1;
            done += len;
        }
    }

    fn render_block(&mut self, n: usize) {
        if self.playing {
            self.schedule(n);
        }

        for t in &mut self.tracks {
            t.used = false;
        }
        let master = &mut self.tracks[MASTER_TRACK];
        master.left[..n].fill(0.0);
        master.right[..n].fill(0.0);

        for slot in 0..self.channels.len() {
            let Some(ch) = self.channels[slot].as_mut() else {
                self.events[slot].clear();
                continue;
            };
            let events = &mut self.events[slot];
            events.sort_unstable_by_key(|e| (e.offset, event_rank(&e.event)));
            let (l, r) = (&mut self.scratch_l[..n], &mut self.scratch_r[..n]);
            l.fill(0.0);
            r.fill(0.0);
            ch.instrument.process(events, l, r);
            events.clear();

            let track = &mut self.tracks[ch.track];
            if !track.used && ch.track != MASTER_TRACK {
                track.left[..n].fill(0.0);
                track.right[..n].fill(0.0);
            }
            track.used = true;
            let (mut pl, mut pr) = (0.0f32, 0.0f32);
            for i in 0..n {
                let gain = ch.volume.next() * ch.mute.next();
                let (gl, gr) = balance_pan(ch.pan.next());
                let (yl, yr) = (l[i] * gain * gl, r[i] * gain * gr);
                pl = pl.max(yl.abs());
                pr = pr.max(yr.abs());
                track.left[i] += yl;
                track.right[i] += yr;
            }
            self.shared.channel_meters[slot].feed(pl, pr);
        }

        // Inserts → effects → fader → master. Tracks with effects run even without input so
        // reverb and delay tails ring out.
        let (master_slice, inserts) = self.tracks.split_at_mut(1);
        let master = &mut master_slice[0];
        for (idx, t) in inserts.iter_mut().enumerate() {
            if !t.used {
                if !t.has_fx() {
                    continue;
                }
                t.left[..n].fill(0.0);
                t.right[..n].fill(0.0);
            }
            t.run_fx(n);
            let (mut pl, mut pr) = (0.0f32, 0.0f32);
            for i in 0..n {
                let gain = t.volume.next() * t.mute.next();
                let (gl, gr) = balance_pan(t.pan.next());
                let (yl, yr) = (t.left[i] * gain * gl, t.right[i] * gain * gr);
                pl = pl.max(yl.abs());
                pr = pr.max(yr.abs());
                master.left[i] += yl;
                master.right[i] += yr;
            }
            self.shared.track_meters[idx + 1].feed(pl, pr);
        }

        master.run_fx(n);
        let (mut pl, mut pr) = (0.0f32, 0.0f32);
        for i in 0..n {
            let gain = master.volume.next() * master.mute.next();
            let (gl, gr) = balance_pan(master.pan.next());
            let yl = sanitize(master.left[i] * gain * gl);
            let yr = sanitize(master.right[i] * gain * gr);
            pl = pl.max(yl.abs());
            pr = pr.max(yr.abs());
            master.left[i] = yl;
            master.right[i] = yr;
        }
        self.metronome.render(&mut master.left[..n], &mut master.right[..n], 0.35);
        self.shared.track_meters[MASTER_TRACK].feed(pl, pr);

        let tick = self.current_tick();
        self.shared.set_position_tick(tick);
    }
}

fn preview_id(slot: usize, key: u8) -> NoteId {
    PREVIEW_ID_BIT | ((slot as u32 & 0x7F_FFFF) << 8) | key as u32
}

/// Ordering of simultaneous events: panic/all-off, then note-offs, then note-ons.
fn event_rank(e: &NoteEvent) -> u8 {
    match e {
        NoteEvent::AllSoundOff | NoteEvent::AllNotesOff => 0,
        NoteEvent::Off { .. } => 1,
        NoteEvent::On { .. } => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::PlaybackNote;
    use soundesti_synthesis::Trident;
    use soundesti_timeline::{PPQ, TempoMap, TimeSignature};

    const SR: f64 = 48_000.0;

    fn engine(max_block: usize) -> (Engine, EngineHandle) {
        Engine::new(EngineConfig { sample_rate: SR, max_block, max_channels: 8 })
    }

    fn note(start: Tick, len: Tick, key: u8) -> PlaybackNote {
        PlaybackNote { channel: 0, start, end: start + len, key, velocity: 0.8 }
    }

    fn snapshot(bpm: f64, notes: Vec<PlaybackNote>) -> Box<PlaybackSnapshot> {
        let ts = TimeSignature::default();
        Box::new(PlaybackSnapshot::new(TempoMap::constant(bpm), ts, 0, ts.ticks_per_bar(), notes))
    }

    /// Renders `frames` stereo frames in device-sized chunks; returns the left channel.
    fn render(e: &mut Engine, frames: usize, chunk: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; frames * 2];
        for c in out.chunks_mut(chunk * 2) {
            e.process(c, 2);
        }
        out.chunks(2).map(|f| f[0]).collect()
    }

    fn setup(max_block: usize, notes: Vec<PlaybackNote>) -> (Engine, EngineHandle) {
        let (mut e, mut h) = engine(max_block);
        h.send(Command::AddChannel { slot: 0, instrument: Box::new(Trident::new(SR as f32)) });
        h.send(Command::ReplaceSnapshot(snapshot(120.0, notes)));
        h.send(Command::Play);
        e.drain_commands();
        (e, h)
    }

    fn first_nonzero(v: &[f32]) -> Option<usize> {
        v.iter().position(|&x| x != 0.0)
    }

    #[test]
    fn note_onset_is_sample_accurate() {
        // Beat 2 at 120 BPM = 0.5 s = sample 24000; block size 512 puts it mid-block.
        let (mut e, _h) = setup(512, vec![note(PPQ, PPQ, 60)]);
        let left = render(&mut e, 30_000, 333);
        let onset = first_nonzero(&left).expect("note produced no sound");
        assert!((24_000..24_004).contains(&onset), "onset at {onset}");
    }

    #[test]
    fn output_is_identical_across_block_sizes() {
        let notes = vec![note(0, 300, 60), note(100, 500, 64), note(PPQ * 2, 37, 67), note(PPQ * 3, PPQ, 48)];
        let (mut a, _ha) = setup(64, notes.clone());
        let (mut b, _hb) = setup(512, notes);
        let la = render(&mut a, 100_000, 64);
        let lb = render(&mut b, 100_000, 1000);
        assert_eq!(la, lb, "rendering must not depend on block size");
    }

    #[test]
    fn pattern_loops() {
        // One-bar loop = 2 s at 120 BPM. A note at tick 0 must retrigger at sample 96000.
        let (mut e, _h) = setup(512, vec![note(0, PPQ / 4, 60)]);
        let left = render(&mut e, 100_000, 512);
        assert!(first_nonzero(&left).unwrap() < 4);
        // Silence before the loop point (note + release long over), sound right after.
        let before = &left[90_000..96_000];
        assert!(before.iter().all(|&x| x.abs() < 1e-6), "tail not silent before loop");
        assert!(left[96_000..97_000].iter().any(|&x| x.abs() > 1e-3), "loop did not retrigger");
    }

    #[test]
    fn short_notes_get_note_offs() {
        // 1-tick notes are far shorter than a block; voices must still be released.
        let notes: Vec<_> = (0..16).map(|i| note(i * 240, 1, 60 + i as u8)).collect();
        let (mut e, mut h) = setup(1024, notes);
        render(&mut e, 96_000 - 2048, 1024);
        h.send(Command::Stop);
        render(&mut e, 48_000, 1024);
        let ch = e.channels[0].as_ref().unwrap();
        assert_eq!(ch.instrument.active_voices(), 0);
    }

    #[test]
    fn stop_releases_and_rewinds() {
        let (mut e, mut h) = setup(512, vec![note(0, PPQ * 4, 60)]);
        render(&mut e, 10_000, 512);
        assert!(e.shared.position_tick() > 0.0);
        h.send(Command::Stop);
        let left = render(&mut e, 48_000, 512);
        assert!(left[40_000..].iter().all(|&x| x == 0.0));
        assert_eq!(e.shared.position_tick(), 0.0);
        assert!(!e.shared.playing.load(std::sync::atomic::Ordering::Relaxed));
    }

    #[test]
    fn preview_note_sounds_while_stopped() {
        let (mut e, mut h) = engine(256);
        h.send(Command::AddChannel { slot: 3, instrument: Box::new(Trident::new(SR as f32)) });
        h.send(Command::NoteOn { slot: 3, key: 72, velocity: 1.0 });
        let left = render(&mut e, 4_800, 256);
        assert!(left.iter().any(|&x| x.abs() > 1e-3));
        h.send(Command::NoteOff { slot: 3, key: 72 });
        let left = render(&mut e, 48_000, 256);
        assert!(left[40_000..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn metronome_clicks_on_beats() {
        let (mut e, mut h) = engine(512);
        h.send(Command::SetMetronome(true));
        h.send(Command::Play);
        let left = render(&mut e, 48_000, 480);
        // 120 BPM: beats at 0 and 24000.
        assert!(left[..2000].iter().any(|&x| x.abs() > 0.05));
        assert!(left[20_000..23_999].iter().all(|&x| x.abs() < 1e-3));
        assert!(left[24_000..26_000].iter().any(|&x| x.abs() > 0.05));
    }

    #[test]
    fn tempo_change_keeps_musical_position() {
        let (mut e, mut h) = setup(512, vec![]);
        render(&mut e, 24_000, 512); // one beat at 120
        let before = e.shared.position_tick();
        h.send(Command::ReplaceSnapshot(snapshot(60.0, vec![])));
        render(&mut e, 0, 512);
        e.drain_commands();
        assert!((e.current_tick() - before).abs() < 1.0, "{} vs {before}", e.current_tick());
        render(&mut e, 48_000, 512); // one beat at 60
        assert!((e.shared.position_tick() - before - PPQ as f64).abs() < 1.0);
    }

    #[test]
    fn mute_fades_to_silence_without_click() {
        let (mut e, mut h) = setup(512, vec![note(0, PPQ * 4, 48)]);
        render(&mut e, 4_800, 512);
        h.send(Command::SetChannel { slot: 0, volume: 0.78, pan: 0.0, muted: true, track: 0 });
        let left = render(&mut e, 4_800, 512);
        let max_step = left.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        assert!(max_step < 0.2, "mute caused a jump of {max_step}");
        assert!(left[1_000..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn removed_instruments_come_back_as_garbage() {
        let (mut e, mut h) = engine(256);
        h.send(Command::AddChannel { slot: 0, instrument: Box::new(Trident::new(SR as f32)) });
        h.send(Command::AddChannel { slot: 0, instrument: Box::new(Trident::new(SR as f32)) });
        h.send(Command::RemoveChannel { slot: 0 });
        h.send(Command::ReplaceSnapshot(snapshot(100.0, vec![])));
        render(&mut e, 256, 256);
        assert_eq!(h.collect_garbage(), 3);
    }

    #[test]
    fn panic_silences_everything() {
        let (mut e, mut h) = setup(256, vec![note(0, PPQ * 4, 60), note(0, PPQ * 4, 64)]);
        render(&mut e, 4_800, 256);
        h.send(Command::Panic);
        let left = render(&mut e, 2_400, 256);
        assert!(left[500..].iter().all(|&x| x == 0.0));
    }

    #[test]
    fn mixer_track_routing_and_meters() {
        let (mut e, mut h) = setup(512, vec![note(0, PPQ * 4, 60)]);
        h.send(Command::SetChannel { slot: 0, volume: 0.78, pan: 0.0, muted: false, track: 5 });
        render(&mut e, 9_600, 512);
        let (l, r) = e.shared.track_meters[5].take();
        assert!(l > 0.0 && r > 0.0, "insert 5 meter silent");
        let (ml, _) = e.shared.track_meters[MASTER_TRACK].take();
        assert!(ml > 0.0);
        // Muting insert 5 silences the master.
        h.send(Command::SetTrack { track: 5, volume: 0.8, pan: 0.0, muted: true });
        let left = render(&mut e, 4_800, 512);
        assert!(left[1_000..].iter().all(|&x| x == 0.0));
    }

    /// Test effect: multiplies by `gain` (param 0).
    struct Gain(f32);
    impl Effect for Gain {
        fn type_id(&self) -> &'static str {
            "test.gain"
        }
        fn display_name(&self) -> &'static str {
            "Gain"
        }
        fn params(&self) -> &'static [soundesti_plugin_api::ParamInfo] {
            &[]
        }
        fn get_param(&self, _id: u32) -> f32 {
            self.0
        }
        fn set_param(&mut self, _id: u32, value: f32) {
            self.0 = value;
        }
        fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
            left.iter_mut().chain(right.iter_mut()).for_each(|x| *x *= self.0);
        }
    }

    #[test]
    fn insert_effects_process_and_bypass() {
        let peak = |v: &[f32]| v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let (mut e, mut h) = setup(512, vec![note(0, PPQ * 8, 60)]);
        h.send(Command::SetChannel { slot: 0, volume: 0.78, pan: 0.0, muted: false, track: 3 });
        let dry = peak(&render(&mut e, 9_600, 512)[2_000..]);
        h.send(Command::SetEffect { track: 3, slot: 0, effect: Some(Box::new(Gain(0.0))) });
        let wet = render(&mut e, 4_800, 512);
        assert!(peak(&wet[1_000..]) == 0.0, "gain-0 insert must silence the track");
        h.send(Command::SetEffectParam { track: 3, slot: 0, id: 0, value: 0.5 });
        let half = peak(&render(&mut e, 4_800, 512)[1_000..]);
        assert!(half > 0.0 && half < dry * 0.8, "half gain: {half} vs dry {dry}");
        h.send(Command::SetEffectBypass { track: 3, slot: 0, bypass: true });
        let bypassed = peak(&render(&mut e, 4_800, 512)[1_000..]);
        assert!(bypassed > half * 1.5, "bypass restores the dry level");
        // Master effects apply too.
        h.send(Command::SetEffect { track: MASTER_TRACK, slot: 9, effect: Some(Box::new(Gain(0.0))) });
        assert!(peak(&render(&mut e, 4_800, 512)[1_000..]) == 0.0);
        h.send(Command::SetEffect { track: 3, slot: 0, effect: None });
        render(&mut e, 512, 512);
        assert!(h.collect_garbage() >= 1, "removed effect is returned for freeing");
    }

    #[test]
    fn deterministic_renders() {
        let notes = vec![note(0, 700, 60), note(PPQ, 700, 63)];
        let (mut a, _) = setup(512, notes.clone());
        let (mut b, _) = setup(512, notes);
        assert_eq!(render(&mut a, 50_000, 512), render(&mut b, 50_000, 512));
    }
}
