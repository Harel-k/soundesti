//! FL Studio project (.flp) import.
//!
//! Brings over what can be represented faithfully: tempo, time signature, channels (name,
//! color, volume, pan, mute, mixer insert), patterns with all notes (velocity, pan,
//! release, fine pitch, color), and the Playlist's pattern clips with track names.
//! FL's own plugins store their sound in private formats that no other program can read,
//! so each channel gets the closest Soundesti instrument instead; everything that could
//! not be imported is listed in [`Imported::report`].

mod events;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use events::{DATA, DWORD, Event, TEXT, WORD, text};
use soundesti_project::{ChannelId, Clip, Note, PatternId, Project};
use soundesti_timeline::{PPQ, TICKS_PER_STEP, Tick, TimeSignature};

#[derive(Debug)]
pub enum FlpError {
    NotFlp,
    Truncated,
    Corrupt(String),
    Io(std::io::Error),
}

impl std::fmt::Display for FlpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlpError::NotFlp => write!(f, "not an FL Studio project (.flp) file"),
            FlpError::Truncated => write!(f, "the file is cut short (incomplete download or copy?)"),
            FlpError::Corrupt(m) => write!(f, "the file is damaged: {m}"),
            FlpError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for FlpError {}

pub struct Imported {
    pub project: Project,
    /// FL Studio version that saved the file, e.g. "21.2.3.4004".
    pub fl_version: String,
    /// What was approximated or skipped, in plain words.
    pub report: Vec<String>,
}

// Event IDs (see events.rs for the size classes).
const CH_ENABLED: u8 = 0;
const CH_VOL_BYTE: u8 = 2;
const CH_PAN_BYTE: u8 = 3;
const TIME_SIG_NUM: u8 = 17;
const TIME_SIG_BEAT: u8 = 18;
const CH_TYPE: u8 = 21;
const CH_ROUTED_TO: u8 = 22;
const CH_NEW: u8 = WORD;
const PAT_NEW: u8 = WORD + 1;
const TEMPO_COARSE: u8 = WORD + 2;
const CH_VOL_WORD: u8 = WORD + 8;
const CH_PAN_WORD: u8 = WORD + 9;
const TEMPO_FINE: u8 = WORD + 29;
const ARR_NEW: u8 = WORD + 35;
const ARR_CURRENT: u8 = WORD + 36;
const PLUGIN_COLOR: u8 = DWORD;
const PAT_COLOR: u8 = DWORD + 22;
const TEMPO: u8 = DWORD + 28;
const CH_NAME_OLD: u8 = TEXT;
const PAT_NAME: u8 = TEXT + 1;
const TITLE: u8 = TEXT + 2;
const SAMPLE_PATH: u8 = TEXT + 4;
const FL_VERSION: u8 = TEXT + 7;
const PLUGIN_INTERNAL: u8 = TEXT + 9;
const PLUGIN_NAME: u8 = TEXT + 11;
const CH_LEVELS: u8 = DATA + 11;
const PAT_NOTES: u8 = DATA + 16;
const PLAYLIST: u8 = DATA + 25;
const TRACK_DATA: u8 = DATA + 30;
const TRACK_NAME: u8 = TEXT + 47;

const PATTERN_BASE: u16 = 20480;
const MAX_TRACK_RVIDX: i64 = 499;

#[derive(Default)]
struct FlChannel {
    iid: u16,
    kind: u8,
    name: Option<String>,
    internal: String,
    sample: Option<String>,
    volume: Option<f32>,
    pan: Option<f32>,
    enabled: bool,
    insert: Option<i8>,
    color: Option<[u8; 3]>,
}

#[derive(Default)]
struct FlPattern {
    name: Option<String>,
    color: Option<[u8; 3]>,
    notes: Vec<(u16, Note)>,
}

struct FlClip {
    position: i64,
    item: u16,
    length: i64,
    track: i64,
    start_offset: f32,
}

/// Imports an .flp file. `path` is also used to find samples stored next to the project.
pub fn import_file(path: &Path) -> Result<Imported, FlpError> {
    let data = std::fs::read(path).map_err(FlpError::Io)?;
    import(&data, path.parent())
}

pub fn import(data: &[u8], project_dir: Option<&Path>) -> Result<Imported, FlpError> {
    let (header, events) = events::parse(data)?;
    let ppq = header.ppq as i64;
    let conv = |t: i64| -> Tick { t * PPQ / ppq };

    let mut version = String::new();
    let mut utf16 = true;
    let mut title = None;
    let mut tempo: Option<f64> = None;
    let (mut coarse, mut fine) = (None, 0u32);
    let mut ts = TimeSignature::default();
    let mut channels: Vec<FlChannel> = Vec::new();
    let mut patterns: Vec<(u16, FlPattern)> = Vec::new();
    let mut arrangements: Vec<(u16, Vec<FlClip>)> = Vec::new();
    let mut current_arr: Option<u16> = None;
    let mut track_names: HashMap<usize, String> = HashMap::new();
    let mut track_colors: HashMap<usize, [u8; 3]> = HashMap::new();
    let mut last_track: Option<usize> = None;
    // Plugin/name events belong to the most recent channel until the arrangement and
    // mixer sections start (mixer effect slots reuse the same plugin event IDs).
    let mut in_channels = false;
    let mut cur_pat: Option<usize> = None;

    for e in &events {
        let Event { id, .. } = *e;
        match id {
            FL_VERSION => {
                version = text(e.bytes(), false);
                let mut it = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
                let (major, minor) = (it.next().unwrap_or(0), it.next().unwrap_or(0));
                utf16 = (major, minor) >= (11, 5);
            }
            TITLE => title = Some(text(e.bytes(), utf16)).filter(|t| !t.is_empty()),
            TEMPO => tempo = Some(e.int() as f64 / 1000.0),
            TEMPO_COARSE => coarse = Some(e.int()),
            TEMPO_FINE => fine = e.int(),
            TIME_SIG_NUM if e.int() > 0 => ts.numerator = e.int().min(16) as u8,
            TIME_SIG_BEAT if [2, 4, 8, 16].contains(&e.int()) => ts.denominator = e.int() as u8,
            CH_NEW => {
                in_channels = true;
                channels.push(FlChannel { iid: e.int() as u16, enabled: true, ..Default::default() });
            }
            PAT_NEW => {
                let iid = e.int() as u16;
                cur_pat = Some(match patterns.iter().position(|(i, _)| *i == iid) {
                    Some(i) => i,
                    None => {
                        patterns.push((iid, FlPattern::default()));
                        patterns.len() - 1
                    }
                });
            }
            PAT_NAME => {
                if let Some(i) = cur_pat {
                    patterns[i].1.name = Some(text(e.bytes(), utf16)).filter(|t| !t.is_empty());
                }
            }
            PAT_COLOR => {
                if let Some(i) = cur_pat {
                    patterns[i].1.color = Some(rgb(e.int()));
                }
            }
            PAT_NOTES => {
                if let Some(i) = cur_pat {
                    patterns[i].1.notes.extend(e.bytes().as_chunks::<24>().0.iter().map(|n| parse_note(n, &conv)));
                }
            }
            ARR_NEW => {
                in_channels = false;
                arrangements.push((e.int() as u16, Vec::new()));
            }
            ARR_CURRENT => current_arr = Some(e.int() as u16),
            PLAYLIST => {
                in_channels = false;
                if arrangements.is_empty() {
                    arrangements.push((0, Vec::new()));
                }
                let b = e.bytes();
                let stride = if !b.is_empty() && b.len() % 60 == 0 && b.len() % 32 != 0 {
                    60
                } else if b.len() % 32 == 0 {
                    32
                } else {
                    60
                };
                let clips = &mut arrangements.last_mut().unwrap().1;
                clips.extend(b.chunks_exact(stride).map(|r| FlClip {
                    position: u32le(r, 0) as i64,
                    item: u16le(r, 6),
                    length: u32le(r, 8) as i64,
                    track: MAX_TRACK_RVIDX - u16le(r, 12) as i64,
                    start_offset: f32::from_bits(u32le(r, 24)),
                }));
            }
            TRACK_DATA => {
                in_channels = false;
                let b = e.bytes();
                if b.len() >= 8 {
                    let idx = (u32le(b, 0) as usize).saturating_sub(1).min(MAX_TRACK_RVIDX as usize);
                    last_track = Some(idx);
                    let c = rgb(u32le(b, 4));
                    if c != [0, 0, 0] {
                        track_colors.insert(idx, c);
                    }
                }
            }
            TRACK_NAME => {
                if let Some(t) = last_track {
                    let name = text(e.bytes(), utf16);
                    if !name.is_empty() {
                        track_names.insert(t, name);
                    }
                }
            }
            _ if in_channels => {
                let Some(ch) = channels.last_mut() else { continue };
                match id {
                    CH_ENABLED => ch.enabled = e.int() != 0,
                    CH_TYPE => ch.kind = e.int() as u8,
                    CH_ROUTED_TO => ch.insert = Some(e.int() as u8 as i8),
                    CH_VOL_BYTE => ch.volume = ch.volume.or(Some(e.int() as f32 / 128.0)),
                    CH_PAN_BYTE => ch.pan = ch.pan.or(Some((e.int() as f32 - 64.0) / 64.0)),
                    CH_VOL_WORD => ch.volume = Some(e.int() as f32 / 12_800.0),
                    CH_PAN_WORD => ch.pan = Some((e.int() as f32 - 6_400.0) / 6_400.0),
                    CH_LEVELS => {
                        let b = e.bytes();
                        if b.len() >= 8 {
                            ch.pan = Some((u32le(b, 0) as i32 as f32 - 6_400.0) / 6_400.0);
                            ch.volume = Some(u32le(b, 4) as f32 / 12_800.0);
                        }
                    }
                    PLUGIN_COLOR => ch.color = Some(rgb(e.int())),
                    CH_NAME_OLD => {
                        let n = text(e.bytes(), utf16);
                        if ch.name.is_none() && !n.is_empty() {
                            ch.name = Some(n);
                        }
                    }
                    PLUGIN_NAME => ch.name = Some(text(e.bytes(), utf16)).filter(|t| !t.is_empty()).or(ch.name.take()),
                    PLUGIN_INTERNAL => ch.internal = text(e.bytes(), utf16),
                    SAMPLE_PATH => ch.sample = Some(text(e.bytes(), utf16)).filter(|t| !t.is_empty()),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    let mut report = Vec::new();
    let mut project = Project::empty();
    project.bpm = tempo
        .or(coarse.map(|c| c as f64 + fine as f64 / 1000.0))
        .filter(|b| (10.0..=999.0).contains(b))
        .unwrap_or(130.0);
    project.time_signature = ts;
    if let Some(t) = title {
        project.meta.title = t;
    }

    // Channels.
    let mut ch_map: HashMap<u16, ChannelId> = HashMap::new();
    let mut remarks = Vec::new();
    let (mut automation, mut layers) = (0, 0);
    for fc in &channels {
        match fc.kind {
            5 => {
                automation += 1;
                continue;
            }
            3 => {
                layers += 1;
                continue;
            }
            _ => {}
        }
        let display = fc.name.clone().unwrap_or_else(|| {
            if fc.internal.is_empty() { format!("Channel {}", fc.iid + 1) } else { fc.internal.clone() }
        });
        let mapped = map_instrument(fc, &display, project_dir);
        remarks.extend(mapped.remark);
        let id = project.add_channel(&display, mapped.type_id);
        let c = project.channel_mut(id).unwrap();
        for (k, v) in mapped.params {
            c.instrument.params.insert(k.to_string(), v);
        }
        c.instrument.sample = mapped.sample;
        c.volume = fc.volume.map(|v| v.clamp(0.0, 1.25)).unwrap_or(0.78);
        c.pan = fc.pan.map(|p| p.clamp(-1.0, 1.0)).unwrap_or(0.0);
        c.muted = !fc.enabled || mapped.muted;
        if let Some(i) = fc.insert.filter(|i| *i > 0) {
            c.mixer_track = (i as u16).min(125);
        }
        if let Some(col) = fc.color.filter(|c| c.iter().any(|x| *x >= 20)) {
            c.color = col;
        }
        ch_map.insert(fc.iid, id);
    }
    summarize(remarks, &mut report);
    if automation > 0 {
        report.push(format!("{automation} automation channel(s) skipped: automation clips are not supported yet."));
    }
    if layers > 0 {
        report.push(format!(
            "{layers} layer channel(s) skipped: layers are not supported yet (their child channels were imported)."
        ));
    }

    // Patterns, in FL's pattern order; the empty project's first pattern is reused.
    patterns.sort_by_key(|(iid, _)| *iid);
    let mut pat_map: HashMap<u16, PatternId> = HashMap::new();
    let mut dropped_notes = 0usize;
    for (n, (iid, fp)) in patterns.iter().enumerate() {
        let pid = if n == 0 { project.patterns[0].id } else { project.add_pattern() };
        let p = project.pattern_mut(pid).unwrap();
        p.name = fp.name.clone().unwrap_or_else(|| format!("Pattern {iid}"));
        p.color = fp.color.filter(|c| c.iter().any(|x| *x >= 20));
        for (rack, note) in &fp.notes {
            match ch_map.get(rack) {
                Some(ch) => p.notes_mut(*ch).push(*note),
                None => dropped_notes += 1,
            }
        }
        for pc in &mut p.channels {
            pc.notes.sort_by_key(|n| (n.start, n.key));
        }
        pat_map.insert(*iid, pid);
    }
    if dropped_notes > 0 {
        report.push(format!("{dropped_notes} note(s) belonged to skipped channels and were left out."));
    }
    if let Some(first) = patterns.first().and_then(|(iid, _)| pat_map.get(iid)) {
        project.current_pattern = *first;
    }

    // Playlist: the current arrangement, else the first one that has clips.
    let arr = current_arr
        .and_then(|c| arrangements.iter().find(|(i, _)| *i == c))
        .or_else(|| arrangements.iter().find(|(_, clips)| !clips.is_empty()));
    let mut other_items = 0;
    if let Some((_, clips)) = arr {
        for fc in clips {
            if fc.item <= PATTERN_BASE {
                other_items += 1;
                continue;
            }
            let Some(pid) = pat_map.get(&(fc.item - PATTERN_BASE)) else { continue };
            let track = fc.track.clamp(0, MAX_TRACK_RVIDX) as u16;
            let offset =
                if fc.start_offset.is_finite() && fc.start_offset > 0.0 { conv(fc.start_offset as i64) } else { 0 };
            project.playlist.ensure_track(track as usize);
            project.playlist.clips.push(Clip {
                pattern: *pid,
                track,
                start: conv(fc.position),
                length: conv(fc.length).max(1),
                offset,
                muted: false,
                selected: false,
            });
        }
    }
    if other_items > 0 {
        report.push(format!(
            "{other_items} audio/automation clip(s) in the Playlist skipped: only pattern clips are supported yet."
        ));
    }
    if arrangements.len() > 1 {
        report.push(format!("The project has {} arrangements; only the current one was imported.", arrangements.len()));
    }
    for (idx, name) in track_names {
        project.playlist.ensure_track(idx);
        project.playlist.tracks[idx].name = name;
    }
    for (idx, c) in track_colors {
        project.playlist.ensure_track(idx);
        project.playlist.tracks[idx].color = Some(c);
    }
    project.song_mode = !project.playlist.clips.is_empty();
    report.push(
        "Mixer settings and effects were not imported (FL effect plugins can't be read outside FL Studio).".into(),
    );
    project.validate();
    Ok(Imported { project, fl_version: version, report })
}

fn parse_note(b: &[u8], conv: &impl Fn(i64) -> Tick) -> (u16, Note) {
    let length = u32le(b, 8) as i64;
    let mut n = Note::new(
        conv(u32le(b, 0) as i64),
        if length == 0 { TICKS_PER_STEP } else { conv(length) },
        u16le(b, 12).min(127) as u8,
    );
    n.fine_pitch = (b[16] as f32 - 120.0) * 10.0;
    n.release = b[18] as f32 / 128.0;
    n.color = b[19] & 0x0f;
    n.pan = ((b[20] as f32 - 64.0) / 64.0).clamp(-1.0, 1.0);
    n.velocity = (b[21] as f32 / 128.0).clamp(0.0, 1.0);
    (u16le(b, 6), n)
}

/// What the importer had to change about one channel.
enum Remark {
    /// An FL plugin replaced by the closest Soundesti instrument.
    Plugin {
        plugin: String,
        target: &'static str,
    },
    /// A controller plugin (no sound of its own), imported muted.
    Controller {
        plugin: String,
    },
    SampleMissing {
        channel: String,
        replaced: bool,
    },
    EmptySampler {
        channel: String,
    },
}

struct Mapped {
    type_id: &'static str,
    params: Vec<(&'static str, f32)>,
    sample: Option<String>,
    muted: bool,
    remark: Option<Remark>,
}

/// Chooses the closest Soundesti instrument for an FL channel.
fn map_instrument(fc: &FlChannel, name: &str, project_dir: Option<&Path>) -> Mapped {
    let m = |type_id, params, sample, remark| Mapped { type_id, params, sample, muted: false, remark };
    const SAMPLER: &str = "soundesti.sampler";
    let lname = name.to_lowercase();
    let internal = fc.internal.to_lowercase();
    let drum_guess = || -> Option<(&'static str, Vec<(&'static str, f32)>)> {
        if lname.contains("kick") || lname.contains("bd") || lname.contains("808") {
            Some(("soundesti.kickr", vec![]))
        } else if lname.contains("clap") {
            Some(("soundesti.clapper", vec![("mode.claps", 3.0)]))
        } else if lname.contains("snare") || lname.contains("rim") {
            Some(("soundesti.clapper", vec![("mode.claps", 0.0)]))
        } else if lname.contains("hat") || lname.contains("hh") || lname.contains("cymbal") || lname.contains("ride") {
            let open = lname.contains("open") || lname.contains("ride") || lname.contains("cymbal");
            Some(("soundesti.hatz", if open { vec![("amp.decay", 0.5)] } else { vec![] }))
        } else {
            None
        }
    };

    let is_sampler = fc.kind == 0 || (fc.internal.is_empty() && fc.sample.is_some());
    if is_sampler {
        let Some(raw) = &fc.sample else {
            return match drum_guess() {
                Some((t, p)) => m(t, p, None, None),
                None => m(SAMPLER, vec![], None, Some(Remark::EmptySampler { channel: name.to_string() })),
            };
        };
        if let Some(found) = resolve_sample(raw, project_dir) {
            return m(SAMPLER, vec![], Some(found.display().to_string()), None);
        }
        return match drum_guess() {
            Some((t, p)) => m(t, p, None, Some(Remark::SampleMissing { channel: name.to_string(), replaced: true })),
            None => m(
                SAMPLER,
                vec![],
                Some(raw.clone()),
                Some(Remark::SampleMissing { channel: name.to_string(), replaced: false }),
            ),
        };
    }

    let plugin = if internal == "fruity wrapper" { format!("{name} (VST)") } else { fc.internal.clone() };
    let approx = |target: &'static str| Some(Remark::Plugin { plugin: plugin.clone(), target });
    match internal.as_str() {
        "3x osc" => m("soundesti.trident", vec![], None, None),
        "fruit kick" | "fruity kick" | "bassdrum" => m("soundesti.kickr", vec![], None, None),
        "fruity drumsynth live" | "drumaxx" => m("soundesti.kickr", vec![], None, approx("Kickr")),
        "plucked!" => m("soundesti.harpa", vec![], None, None),
        "sytrus" | "fruity dx10" | "fl keys" | "toxic biohazard" => {
            m("soundesti.orbit", vec![], None, approx("Orbit (FM)"))
        }
        "boobass" | "transistor bass" => {
            m("soundesti.trident", vec![("filter.cutoff", 700.0)], None, approx("Trident (bass)"))
        }
        "fruity envelope controller"
        | "fruity keyboard controller"
        | "fruity x-y controller"
        | "fruity formula controller"
        | "patcher" => Mapped {
            type_id: "soundesti.trident",
            params: vec![],
            sample: None,
            muted: true,
            remark: Some(Remark::Controller { plugin: fc.internal.clone() }),
        },
        _ => match drum_guess() {
            Some((t, p)) => m(t, p, None, approx(soundesti_name(t))),
            None => m("soundesti.trident", vec![], None, approx("Trident")),
        },
    }
}

fn soundesti_name(type_id: &str) -> &'static str {
    match type_id {
        "soundesti.kickr" => "Kickr",
        "soundesti.clapper" => "Clapper",
        "soundesti.hatz" => "Hatz",
        _ => "Trident",
    }
}

/// Turns per-channel remarks into a short, grouped report.
fn summarize(remarks: Vec<Remark>, report: &mut Vec<String>) {
    let mut plugins: Vec<(String, &'static str, usize)> = Vec::new();
    let mut controllers: Vec<String> = Vec::new();
    let (mut missing, mut replaced, mut empty): (Vec<String>, usize, Vec<String>) = (Vec::new(), 0, Vec::new());
    for r in remarks {
        match r {
            Remark::Plugin { plugin, target } => {
                match plugins.iter_mut().find(|(p, t, _)| *p == plugin && *t == target) {
                    Some(e) => e.2 += 1,
                    None => plugins.push((plugin, target, 1)),
                }
            }
            Remark::Controller { plugin } => controllers.push(plugin),
            Remark::SampleMissing { channel, replaced: r } => {
                replaced += r as usize;
                missing.push(channel);
            }
            Remark::EmptySampler { channel } => empty.push(channel),
        }
    }
    plugins.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    let examples = |v: &[String]| {
        let mut s = v.iter().take(3).map(|n| format!("\"{n}\"")).collect::<Vec<_>>().join(", ");
        if v.len() > 3 {
            s.push_str(&format!(" and {} more", v.len() - 3));
        }
        s
    };
    for (plugin, target, n) in plugins {
        let chans = if n == 1 { "1 channel".to_string() } else { format!("{n} channels") };
        report.push(format!("FL plugin \"{plugin}\" ({chans}) -> {target}, the closest Soundesti instrument."));
    }
    if !controllers.is_empty() {
        report.push(format!(
            "{} controller channel(s) imported muted (they make no sound of their own).",
            controllers.len()
        ));
    }
    if !missing.is_empty() {
        let tail = if replaced > 0 {
            format!("; {replaced} drum(s) replaced with Soundesti drum synths")
        } else {
            String::new()
        };
        report.push(format!(
            "{} sample(s) not found on this PC: {}{tail}. Use Replace in the channel to pick a file.",
            missing.len(),
            examples(&missing)
        ));
    }
    if !empty.is_empty() {
        report.push(format!("{} empty sampler channel(s): {}.", empty.len(), examples(&empty)));
    }
}

/// FL writes paths with placeholders like `%FLStudioFactoryData%`; look for the file in
/// the usual places, then next to the project.
fn resolve_sample(raw: &str, project_dir: Option<&Path>) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    if raw.contains('%') {
        let tail = |key: &str| raw.find(key).map(|i| raw[i + key.len()..].trim_start_matches(['\\', '/']).to_string());
        if let Some(rest) = tail("%FLStudioFactoryData%") {
            for base in fl_install_dirs() {
                candidates.push(base.join(&rest));
            }
        }
        if let Some(rest) = tail("%FLStudioUserData%")
            && let Some(home) = std::env::var_os("USERPROFILE")
        {
            candidates.push(PathBuf::from(home).join("Documents").join("Image-Line").join("FL Studio").join(rest));
        }
    } else {
        candidates.push(PathBuf::from(raw));
    }
    if let (Some(dir), Some(file)) = (project_dir, Path::new(&raw.replace('\\', "/")).file_name()) {
        candidates.push(dir.join(file));
    }
    candidates.into_iter().find(|p| p.is_file())
}

fn fl_install_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    for root in ["C:\\Program Files\\Image-Line", "C:\\Program Files (x86)\\Image-Line"] {
        if let Ok(rd) = std::fs::read_dir(root) {
            out.extend(
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("FL Studio"))),
            );
        }
    }
    out
}

fn rgb(v: u32) -> [u8; 3] {
    [(v & 0xff) as u8, (v >> 8 & 0xff) as u8, (v >> 16 & 0xff) as u8]
}

fn u16le(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[cfg(test)]
mod tests {
    use super::events::writer::Writer;
    use super::*;

    fn note(pos: u32, rack: u16, len: u32, key: u16, vel: u8) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&pos.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&rack.to_le_bytes());
        b.extend_from_slice(&len.to_le_bytes());
        b.extend_from_slice(&key.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&[120, 0, 64, 0, 32, vel, 128, 128]);
        b
    }

    fn clip(pos: u32, item: u16, len: u32, track: u16, size: usize) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&pos.to_le_bytes());
        b.extend_from_slice(&PATTERN_BASE.to_le_bytes());
        b.extend_from_slice(&item.to_le_bytes());
        b.extend_from_slice(&len.to_le_bytes());
        b.extend_from_slice(&(499 - track).to_le_bytes());
        b.extend_from_slice(&[0, 0, 120, 0, 64, 0, 64, 100, 128, 128]);
        b.extend_from_slice(&(-1f32).to_bits().to_le_bytes());
        b.extend_from_slice(&(-1f32).to_bits().to_le_bytes());
        b.resize(size, 0);
        b
    }

    /// A small FL 21-style project at PPQ 96: two channels, two patterns, three clips.
    fn sample_project(clip_size: usize) -> Vec<u8> {
        let mut w = Writer::new(96);
        w.ascii(FL_VERSION, "21.2.3.4004")
            .utf16(TITLE, "My Beat")
            .int(TEMPO, 140_000)
            .int(TIME_SIG_NUM, 4)
            .int(TIME_SIG_BEAT, 4);
        // Patterns (FL writes notes before channels).
        let mut notes = note(0, 0, 0, 60, 100);
        notes.extend(note(96, 0, 0, 60, 128));
        notes.extend(note(0, 1, 192, 64, 64));
        w.int(PAT_NEW, 1).bytes(PAT_NOTES, &notes);
        w.int(PAT_NEW, 2).bytes(PAT_NOTES, &note(48, 1, 48, 67, 100));
        w.int(PAT_NEW, 1).utf16(PAT_NAME, "Drums");
        w.int(PAT_NEW, 2).utf16(PAT_NAME, "Lead");
        // Channel 0: a sampler whose file doesn't exist, named Kick.
        w.int(CH_NEW, 0)
            .int(CH_TYPE, 0)
            .utf16(PLUGIN_NAME, "Kick")
            .utf16(SAMPLE_PATH, "%FLStudioFactoryData%\\Data\\Patches\\Packs\\Drums\\Kicks\\Nope Kick.wav");
        let mut levels = Vec::new();
        levels.extend_from_slice(&3_200i32.to_le_bytes()); // pan half left
        levels.extend_from_slice(&6_400u32.to_le_bytes()); // volume 50%
        levels.resize(24, 0);
        w.bytes(CH_LEVELS, &levels).int(CH_ROUTED_TO, 3);
        // Channel 1: 3x Osc, muted.
        w.int(CH_NEW, 1)
            .int(CH_TYPE, 2)
            .utf16(PLUGIN_INTERNAL, "3x Osc")
            .utf16(PLUGIN_NAME, "Lead Synth")
            .int(CH_ENABLED, 0);
        // Channel 2: automation (skipped).
        w.int(CH_NEW, 2).int(CH_TYPE, 5);
        // Arrangement with clips and a named track.
        w.int(ARR_NEW, 0);
        let mut items = clip(0, PATTERN_BASE + 1, 384, 0, clip_size);
        items.extend(clip(384, PATTERN_BASE + 1, 384, 0, clip_size));
        items.extend(clip(384, PATTERN_BASE + 2, 384, 1, clip_size));
        w.bytes(PLAYLIST, &items);
        let mut track = 2u32.to_le_bytes().to_vec();
        track.extend_from_slice(&0x0000_80ffu32.to_le_bytes());
        track.resize(66, 0);
        w.bytes(TRACK_DATA, &track).utf16(TRACK_NAME, "Melody");
        // A mixer effect slot plugin name must not rename the last channel.
        w.utf16(PLUGIN_NAME, "Fruity Limiter");
        w.finish()
    }

    #[test]
    fn imports_a_project() {
        for size in [32, 60] {
            let imp = import(&sample_project(size), None).unwrap();
            let p = &imp.project;
            assert_eq!(imp.fl_version, "21.2.3.4004");
            assert_eq!(p.meta.title, "My Beat");
            assert_eq!(p.bpm, 140.0);
            assert_eq!(p.channels.len(), 2, "automation channel skipped");
            let (kick, lead) = (&p.channels[0], &p.channels[1]);
            assert_eq!(kick.name, "Kick");
            assert_eq!(kick.instrument.type_id, "soundesti.kickr", "missing kick sample -> drum synth");
            assert!((kick.pan + 0.5).abs() < 1e-6 && (kick.volume - 0.5).abs() < 1e-6);
            assert_eq!(kick.mixer_track, 3);
            assert_eq!(lead.name, "Lead Synth");
            assert_eq!(lead.instrument.type_id, "soundesti.trident");
            assert!(lead.muted);

            assert_eq!(p.patterns.len(), 2);
            let drums = &p.patterns[0];
            assert_eq!(drums.name, "Drums");
            let k = drums.notes(kick.id);
            assert_eq!(k.len(), 2);
            assert_eq!((k[0].start, k[0].length, k[0].key), (0, TICKS_PER_STEP, 60), "step note");
            assert_eq!(k[1].start, 960, "PPQ 96 -> 960");
            assert!((k[1].velocity - 1.0).abs() < 1e-6);
            let l = drums.notes(lead.id);
            assert_eq!((l[0].length, l[0].key), (1920, 64));
            assert!((l[0].pan + 0.5).abs() < 1e-6);
            assert_eq!(p.patterns[1].name, "Lead");
            assert_eq!(p.patterns[1].notes(lead.id)[0].start, 480);

            assert!(p.song_mode);
            assert_eq!(p.playlist.clips.len(), 3, "clip size {size}");
            let c = &p.playlist.clips[2];
            assert_eq!((c.track, c.start, c.length, c.pattern), (1, 3840, 3840, p.patterns[1].id));
            assert_eq!(p.playlist.tracks[1].name, "Melody");
            assert_eq!(p.playlist.tracks[1].color, Some([0xff, 0x80, 0]));
            assert!(imp.report.iter().any(|r| r.contains("automation channel")));
            assert!(imp.report.iter().any(|r| r.contains("not found on this PC: \"Kick\"")), "{:?}", imp.report);
        }
    }

    #[test]
    fn old_files_use_ascii_text_and_coarse_tempo() {
        let mut w = Writer::new(96);
        w.ascii(FL_VERSION, "9.0.0").int(TEMPO_COARSE, 128).int(TEMPO_FINE, 500);
        w.int(CH_NEW, 0).int(CH_TYPE, 2).ascii(PLUGIN_INTERNAL, "Plucked!").ascii(CH_NAME_OLD, "Pluck");
        let imp = import(&w.finish(), None).unwrap();
        assert_eq!(imp.project.bpm, 128.5);
        assert_eq!(imp.project.channels[0].name, "Pluck");
        assert_eq!(imp.project.channels[0].instrument.type_id, "soundesti.harpa");
        assert!(!imp.project.song_mode);
    }

    #[test]
    fn samples_next_to_the_project_are_found() {
        let dir = std::env::temp_dir().join(format!("soundesti-flp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("my loop.wav"), b"RIFF").unwrap();
        let mut w = Writer::new(96);
        w.ascii(FL_VERSION, "20.8.0")
            .int(CH_NEW, 0)
            .int(CH_TYPE, 0)
            .utf16(SAMPLE_PATH, "D:\\Old PC\\Samples\\my loop.wav");
        let imp = import(&w.finish(), Some(&dir)).unwrap();
        let ch = &imp.project.channels[0];
        assert_eq!(ch.instrument.type_id, "soundesti.sampler");
        assert_eq!(ch.instrument.sample.as_deref(), Some(dir.join("my loop.wav").display().to_string().as_str()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn garbage_never_panics() {
        let base = sample_project(32);
        for cut in (0..base.len()).step_by(7) {
            let _ = import(&base[..cut], None);
        }
        let mut flipped = base.clone();
        for i in (22..flipped.len()).step_by(13) {
            flipped[i] ^= 0x5a;
            let _ = import(&flipped, None);
        }
    }
}
