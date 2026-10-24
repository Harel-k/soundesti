//! Offline export. Uses exactly the same engine code path as live playback, so
//! exported audio matches what the user hears (verified in tests).

use std::path::{Path, PathBuf};

use soundesti_audio::{Command, EngineConfig};
use soundesti_project::Project;
use soundesti_session::Session;
use soundesti_timeline::TempoMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitDepth {
    Int16,
    Int24,
    Float32,
}

/// What to render (FL's "Pattern" / "Full song").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Pattern,
    Song,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Wav,
    /// Lossless; 16 or 24 bit (from `bit_depth`; 32-bit float becomes 24-bit).
    Flac,
    /// Constant bitrate MP3 (LAME), up to 48 kHz.
    Mp3 {
        kbps: u32,
    },
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Wav => "wav",
            Format::Flac => "flac",
            Format::Mp3 { .. } => "mp3",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    pub range: Range,
    pub format: Format,
    pub sample_rate: u32,
    pub bit_depth: BitDepth,
    /// How many times to play the current pattern.
    pub loops: u32,
    /// Extra time rendered after the last loop so releases and reverb tails ring out.
    pub tail_seconds: f32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            range: Range::Pattern,
            format: Format::Wav,
            sample_rate: 48_000,
            bit_depth: BitDepth::Int24,
            loops: 1,
            tail_seconds: 2.0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("export cancelled")]
    Cancelled,
    #[error("could not write {path}: {message}")]
    Write { path: PathBuf, message: String },
    #[error("invalid export options: {0}")]
    Options(&'static str),
    #[error("encoding failed: {0}")]
    Encode(String),
}

#[derive(Clone, Copy, Debug)]
pub struct ExportReport {
    pub frames: usize,
    pub seconds: f64,
    pub peak: f32,
}

const BLOCK: usize = 512;

/// Renders the current pattern (looped `opts.loops` times) or the whole song to
/// interleaved stereo f32. `progress` receives 0..1 and returns false to cancel.
pub fn render(
    project: &Project,
    opts: &ExportOptions,
    mut progress: impl FnMut(f32) -> bool,
) -> Result<Vec<f32>, ExportError> {
    if opts.sample_rate < 8_000 || opts.sample_rate > 384_000 {
        return Err(ExportError::Options("sample rate must be 8-384 kHz"));
    }
    if opts.loops == 0 {
        return Err(ExportError::Options("loops must be at least 1"));
    }
    if matches!(opts.format, Format::Mp3 { .. }) && opts.sample_rate > 48_000 {
        return Err(ExportError::Options("MP3 supports sample rates up to 48 kHz"));
    }
    let ts = project.time_signature;
    let mut song;
    let (project, len_ticks) = match opts.range {
        Range::Pattern => (project, project.current().length_ticks(ts) * opts.loops as i64),
        Range::Song => {
            song = project.clone();
            song.song_mode = true;
            let len = song.playlist.song_length(ts);
            if song.playlist.clips.is_empty() || len <= 0 {
                return Err(ExportError::Options("the Playlist is empty; add pattern clips or export the pattern"));
            }
            (&song, len)
        }
    };
    let config = EngineConfig { sample_rate: opts.sample_rate as f64, max_block: BLOCK, max_channels: 256 };
    let (mut engine, mut session) = Session::with_engine(project, config);

    let tempo = TempoMap::constant(project.bpm);
    let body = tempo.sample_at_tick(len_ticks as f64, config.sample_rate) as usize;
    let tail = (opts.tail_seconds.max(0.0) as f64 * config.sample_rate) as usize;
    let total = body + tail;

    session.send(Command::Play);
    let mut out = vec![0.0f32; total * 2];
    let mut done = 0usize;
    while done < total {
        if done == body {
            // Stop exactly at the end of the last loop so the pattern does not restart
            // during the tail; notes are released and ring out.
            session.send(Command::Pause);
        }
        let limit = if done < body { body } else { total };
        let n = BLOCK.min(limit - done);
        engine.process(&mut out[done * 2..(done + n) * 2], 2);
        done += n;
        if !progress(done as f32 / total.max(1) as f32) {
            return Err(ExportError::Cancelled);
        }
    }
    Ok(out)
}

/// Renders the current pattern (kept for callers that predate song export).
pub fn render_pattern(
    project: &Project,
    opts: &ExportOptions,
    progress: impl FnMut(f32) -> bool,
) -> Result<Vec<f32>, ExportError> {
    render(project, &ExportOptions { range: Range::Pattern, ..*opts }, progress)
}

/// Writes interleaved stereo samples as WAV. Writes to a temp file first, so a failed
/// export never leaves a truncated file at `path`.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32, depth: BitDepth) -> Result<(), ExportError> {
    let werr = |e: &dyn std::fmt::Display| ExportError::Write { path: path.to_path_buf(), message: e.to_string() };
    let (bits, format) = match depth {
        BitDepth::Int16 => (16, hound::SampleFormat::Int),
        BitDepth::Int24 => (24, hound::SampleFormat::Int),
        BitDepth::Float32 => (32, hound::SampleFormat::Float),
    };
    let spec = hound::WavSpec { channels: 2, sample_rate, bits_per_sample: bits, sample_format: format };
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    {
        let mut w = hound::WavWriter::create(&tmp, spec).map_err(|e| werr(&e))?;
        let mut dither = Tpdf::new();
        for &s in samples {
            let s = if s.is_finite() { s } else { 0.0 };
            match depth {
                BitDepth::Float32 => w.write_sample(s).map_err(|e| werr(&e))?,
                BitDepth::Int16 => {
                    let v = (s * 32767.0 + dither.next()).round().clamp(-32768.0, 32767.0) as i16;
                    w.write_sample(v).map_err(|e| werr(&e))?
                }
                BitDepth::Int24 => {
                    let v = (s * 8_388_607.0).round().clamp(-8_388_608.0, 8_388_607.0) as i32;
                    w.write_sample(v).map_err(|e| werr(&e))?
                }
            }
        }
        w.finalize().map_err(|e| werr(&e))?;
    }
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        werr(&e)
    })
}

/// Writes interleaved stereo samples as FLAC (16 or 24 bit).
pub fn write_flac(path: &Path, samples: &[f32], sample_rate: u32, depth: BitDepth) -> Result<(), ExportError> {
    use flacenc::component::BitRepr;
    use flacenc::error::Verify;
    let bits = if depth == BitDepth::Int16 { 16 } else { 24 };
    let scale = ((1i64 << (bits - 1)) - 1) as f32;
    let mut dither = Tpdf::new();
    let ints: Vec<i32> = samples
        .iter()
        .map(|&s| {
            let s = if s.is_finite() { s } else { 0.0 };
            let d = if bits == 16 { dither.next() } else { 0.0 };
            (s * scale + d).round().clamp(-scale - 1.0, scale) as i32
        })
        .collect();
    let config =
        flacenc::config::Encoder::default().into_verified().map_err(|e| ExportError::Encode(format!("{e:?}")))?;
    let source = flacenc::source::MemSource::from_samples(&ints, 2, bits, sample_rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| ExportError::Encode(format!("{e:?}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream.write(&mut sink).map_err(|e| ExportError::Encode(format!("{e:?}")))?;
    write_atomic(path, sink.as_slice())
}

/// Writes interleaved stereo samples as constant-bitrate MP3.
pub fn write_mp3(path: &Path, samples: &[f32], sample_rate: u32, kbps: u32) -> Result<(), ExportError> {
    use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, InterleavedPcm, Quality};
    let rate = match kbps {
        0..=96 => Bitrate::Kbps96,
        97..=128 => Bitrate::Kbps128,
        129..=160 => Bitrate::Kbps160,
        161..=192 => Bitrate::Kbps192,
        193..=224 => Bitrate::Kbps224,
        225..=256 => Bitrate::Kbps256,
        _ => Bitrate::Kbps320,
    };
    let enc_err = |e: &dyn std::fmt::Debug| ExportError::Encode(format!("MP3: {e:?}"));
    let mut b = Builder::new().ok_or_else(|| ExportError::Encode("MP3 encoder could not start".into()))?;
    b.set_num_channels(2).map_err(|e| enc_err(&e))?;
    b.set_sample_rate(sample_rate).map_err(|e| enc_err(&e))?;
    b.set_brate(rate).map_err(|e| enc_err(&e))?;
    b.set_quality(Quality::Best).map_err(|e| enc_err(&e))?;
    let mut enc = b.build().map_err(|e| enc_err(&e))?;
    let clean: Vec<f32> = samples.iter().map(|s| if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 }).collect();
    let mut out = Vec::with_capacity(clean.len() / 4);
    for chunk in clean.chunks(2 * 8192) {
        out.reserve(mp3lame_encoder::max_required_buffer_size(chunk.len() / 2));
        enc.encode_to_vec(InterleavedPcm(chunk), &mut out).map_err(|e| enc_err(&e))?;
    }
    out.reserve(7200);
    enc.flush_to_vec::<FlushNoGap>(&mut out).map_err(|e| enc_err(&e))?;
    write_atomic(path, &out)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), ExportError> {
    let werr = |e: &dyn std::fmt::Display| ExportError::Write { path: path.to_path_buf(), message: e.to_string() };
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".part");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes).map_err(|e| werr(&e))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        werr(&e)
    })
}

/// Writes samples in the format chosen in `opts`.
pub fn write_file(path: &Path, samples: &[f32], opts: &ExportOptions) -> Result<(), ExportError> {
    match opts.format {
        Format::Wav => write_wav(path, samples, opts.sample_rate, opts.bit_depth),
        Format::Flac => write_flac(path, samples, opts.sample_rate, opts.bit_depth),
        Format::Mp3 { kbps } => write_mp3(path, samples, opts.sample_rate, kbps),
    }
}

fn report(samples: &[f32], sample_rate: u32) -> ExportReport {
    let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    let frames = samples.len() / 2;
    ExportReport { frames, seconds: frames as f64 / sample_rate as f64, peak }
}

/// Renders and writes one file.
pub fn export(
    project: &Project,
    path: &Path,
    opts: &ExportOptions,
    progress: impl FnMut(f32) -> bool,
) -> Result<ExportReport, ExportError> {
    let samples = render(project, opts, progress)?;
    write_file(path, &samples, opts)?;
    Ok(report(&samples, opts.sample_rate))
}

/// Renders the current pattern and writes it as WAV (kept for older callers).
pub fn export_wav(
    project: &Project,
    path: &Path,
    opts: &ExportOptions,
    progress: impl FnMut(f32) -> bool,
) -> Result<ExportReport, ExportError> {
    export(project, path, &ExportOptions { range: Range::Pattern, format: Format::Wav, ..*opts }, progress)
}

/// FL's "Split mixer tracks": one file per mixer track that has channels routed to it,
/// named "<base> - <track name>.<ext>" next to `path`. Returns the files written.
pub fn export_stems(
    project: &Project,
    path: &Path,
    opts: &ExportOptions,
    mut progress: impl FnMut(f32) -> bool,
) -> Result<Vec<PathBuf>, ExportError> {
    let mut tracks: Vec<u16> = project.channels.iter().filter(|c| !c.muted).map(|c| c.mixer_track).collect();
    tracks.sort_unstable();
    tracks.dedup();
    if tracks.is_empty() {
        return Err(ExportError::Options("no unmuted channels to export"));
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "Export".into());
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut written = Vec::new();
    let n = tracks.len();
    for (i, t) in tracks.into_iter().enumerate() {
        let mut p = project.clone();
        for c in &mut p.channels {
            c.muted |= c.mixer_track != t;
        }
        let name = project.mixer.get(t as usize).map(|m| m.name.clone()).unwrap_or_else(|| format!("Track {t}"));
        let clean: String = name.chars().map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c }).collect();
        let file = dir.join(format!("{stem} - {clean}.{}", opts.format.extension()));
        let samples = render(&p, opts, |f| progress((i as f32 + f) / n as f32))?;
        write_file(&file, &samples, opts)?;
        written.push(file);
    }
    Ok(written)
}

/// Triangular-PDF dither of +-1 LSB for 16-bit output.
struct Tpdf {
    state: u32,
}

impl Tpdf {
    fn new() -> Self {
        Self { state: 0x1234_5678 }
    }

    fn uniform(&mut self) -> f32 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        x as f32 / u32::MAX as f32
    }

    fn next(&mut self) -> f32 {
        self.uniform() - self.uniform()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soundesti_project::Note;

    fn project_with_notes() -> Project {
        let mut p = Project::new();
        p.bpm = 120.0;
        let ch = p.channels[0].id;
        let notes = p.current_mut().notes_mut(ch);
        notes.push(Note::new(0, 480, 60));
        notes.push(Note::new(1920, 480, 67));
        p
    }

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("soundesti-export-{name}-{}.wav", std::process::id()))
    }

    #[test]
    fn wav_has_expected_length_and_content() {
        let p = project_with_notes();
        let path = temp("len");
        let opts = ExportOptions {
            sample_rate: 44_100,
            bit_depth: BitDepth::Int24,
            loops: 2,
            tail_seconds: 1.0,
            ..Default::default()
        };
        let report = export_wav(&p, &path, &opts, |_| true).unwrap();
        // 2 loops of one 4/4 bar at 120 BPM = 4 s, plus 1 s tail.
        assert_eq!(report.frames, 5 * 44_100);
        assert!(report.peak > 0.01 && report.peak < 1.0, "peak {}", report.peak);
        let r = hound::WavReader::open(&path).unwrap();
        assert_eq!(r.spec().sample_rate, 44_100);
        assert_eq!(r.spec().bits_per_sample, 24);
        assert_eq!(r.len() as usize, report.frames * 2);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn export_matches_engine_output_exactly() {
        let p = project_with_notes();
        let opts = ExportOptions {
            sample_rate: 48_000,
            bit_depth: BitDepth::Float32,
            loops: 1,
            tail_seconds: 0.5,
            ..Default::default()
        };
        let rendered = render_pattern(&p, &opts, |_| true).unwrap();
        let path = temp("exact");
        write_wav(&path, &rendered, 48_000, BitDepth::Float32).unwrap();
        let back: Vec<f32> = hound::WavReader::open(&path).unwrap().samples::<f32>().map(Result::unwrap).collect();
        assert_eq!(back, rendered);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn tail_does_not_restart_the_pattern() {
        let p = project_with_notes();
        let opts = ExportOptions {
            sample_rate: 48_000,
            bit_depth: BitDepth::Float32,
            loops: 1,
            tail_seconds: 2.0,
            ..Default::default()
        };
        let s = render_pattern(&p, &opts, |_| true).unwrap();
        // Bar is 2 s; the downbeat note would retrigger at 96000 if the loop kept going.
        let after = &s[(96_000 + 24_000) * 2..];
        assert!(after.iter().all(|x| x.abs() < 1e-4), "pattern restarted during the tail");
    }

    #[test]
    fn cancellation_writes_nothing() {
        let p = project_with_notes();
        let path = temp("cancel");
        let mut calls = 0;
        let res = export_wav(&p, &path, &ExportOptions::default(), |_| {
            calls += 1;
            calls < 5
        });
        assert!(matches!(res, Err(ExportError::Cancelled)));
        assert!(!path.exists());
    }

    #[test]
    fn rejects_bad_options() {
        let p = Project::new();
        let bad = ExportOptions { sample_rate: 10, ..Default::default() };
        assert!(matches!(render_pattern(&p, &bad, |_| true), Err(ExportError::Options(_))));
        let bad = ExportOptions { loops: 0, ..Default::default() };
        assert!(matches!(render_pattern(&p, &bad, |_| true), Err(ExportError::Options(_))));
    }

    #[test]
    fn sixteen_bit_clips_safely() {
        let path = temp("clip16");
        write_wav(&path, &[2.0, -2.0, f32::NAN, 0.5], 48_000, BitDepth::Int16).unwrap();
        let v: Vec<i16> = hound::WavReader::open(&path).unwrap().samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(v[0], 32767);
        assert_eq!(v[1], -32768);
        assert!(v[2].abs() <= 1);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn song_export_covers_the_arrangement() {
        let mut p = project_with_notes();
        let pid = p.current_pattern;
        // Clips on bars 1 and 3 -> 3 bars of song (6 s at 120 BPM).
        for bar in [0, 2] {
            p.playlist.clips.push(soundesti_project::Clip {
                pattern: pid,
                track: 0,
                start: bar * 3840,
                length: 3840,
                offset: 0,
                muted: false,
                selected: false,
            });
        }
        let opts = ExportOptions { range: Range::Song, sample_rate: 48_000, tail_seconds: 0.0, ..Default::default() };
        let s = render(&p, &opts, |_| true).unwrap();
        assert_eq!(s.len() / 2, 6 * 48_000);
        let loud = |a: usize, b: usize| s[a * 2..b * 2].iter().any(|x| x.abs() > 1e-3);
        assert!(loud(0, 4_800) && !loud(96_000 + 48_000, 192_000) && loud(192_000, 196_800));
        p.playlist.clips.clear();
        assert!(matches!(render(&p, &opts, |_| true), Err(ExportError::Options(_))));
    }

    #[test]
    fn flac_and_mp3_files_are_valid() {
        let p = project_with_notes();
        for format in [Format::Flac, Format::Mp3 { kbps: 192 }] {
            let path = temp(format.extension()).with_extension(format.extension());
            let opts = ExportOptions { format, sample_rate: 44_100, bit_depth: BitDepth::Int16, ..Default::default() };
            export(&p, &path, &opts, |_| true).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            match format {
                Format::Flac => assert_eq!(&bytes[..4], b"fLaC"),
                _ => assert!(
                    bytes.windows(2).take(4096).any(|w| w[0] == 0xFF && w[1] & 0xE0 == 0xE0),
                    "no MP3 frame sync"
                ),
            }
            assert!(bytes.len() > 5_000, "{format:?} too small: {}", bytes.len());
            std::fs::remove_file(path).ok();
        }
        let too_fast = ExportOptions { format: Format::Mp3 { kbps: 320 }, sample_rate: 96_000, ..Default::default() };
        assert!(matches!(render(&p, &too_fast, |_| true), Err(ExportError::Options(_))));
    }

    #[test]
    fn stems_split_by_mixer_track() {
        let mut p = project_with_notes();
        let (a, b) = (p.channels[0].id, p.channels[1].id);
        p.channel_mut(a).unwrap().mixer_track = 1;
        p.channel_mut(b).unwrap().mixer_track = 2;
        p.current_mut().notes_mut(b).push(Note::new(0, 480, 60));
        for c in p.channels.iter_mut().skip(2) {
            c.muted = true;
        }
        let dir = std::env::temp_dir().join(format!("soundesti-stems-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let opts = ExportOptions {
            sample_rate: 48_000,
            bit_depth: BitDepth::Float32,
            tail_seconds: 0.5,
            ..Default::default()
        };
        let files = export_stems(&p, &dir.join("Song.wav"), &opts, |_| true).unwrap();
        let names: Vec<String> = files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["Song - Insert 1.wav", "Song - Insert 2.wav"]);
        // Stem 1 has channel a's second note at beat 3; stem 2 does not.
        let at_beat3 = |f: &PathBuf| {
            let s: Vec<f32> = hound::WavReader::open(f).unwrap().samples::<f32>().map(Result::unwrap).collect();
            s[48_000 * 2..(48_000 + 4_800) * 2].iter().any(|x| x.abs() > 1e-3)
        };
        assert!(at_beat3(&files[0]) && !at_beat3(&files[1]));
        std::fs::remove_dir_all(&dir).ok();
    }
}
