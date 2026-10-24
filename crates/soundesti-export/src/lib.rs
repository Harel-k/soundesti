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

#[derive(Clone, Copy, Debug)]
pub struct ExportOptions {
    pub sample_rate: u32,
    pub bit_depth: BitDepth,
    /// How many times to play the current pattern.
    pub loops: u32,
    /// Extra time rendered after the last loop so releases and reverb tails ring out.
    pub tail_seconds: f32,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { sample_rate: 48_000, bit_depth: BitDepth::Int24, loops: 1, tail_seconds: 2.0 }
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
}

#[derive(Clone, Copy, Debug)]
pub struct ExportReport {
    pub frames: usize,
    pub seconds: f64,
    pub peak: f32,
}

const BLOCK: usize = 512;

/// Renders the current pattern to interleaved stereo f32. `progress` receives 0..1 and
/// returns false to cancel.
pub fn render_pattern(
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
    let config = EngineConfig { sample_rate: opts.sample_rate as f64, max_block: BLOCK, max_channels: 256 };
    let (mut engine, mut session) = Session::with_engine(project, config);

    let ts = project.time_signature;
    let len_ticks = project.current().length_ticks(ts) * opts.loops as i64;
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

/// Renders the current pattern and writes it to `path`.
pub fn export_wav(
    project: &Project,
    path: &Path,
    opts: &ExportOptions,
    progress: impl FnMut(f32) -> bool,
) -> Result<ExportReport, ExportError> {
    let samples = render_pattern(project, opts, progress)?;
    write_wav(path, &samples, opts.sample_rate, opts.bit_depth)?;
    let peak = samples.iter().fold(0.0f32, |a, s| a.max(s.abs()));
    let frames = samples.len() / 2;
    Ok(ExportReport { frames, seconds: frames as f64 / opts.sample_rate as f64, peak })
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
        let opts = ExportOptions { sample_rate: 44_100, bit_depth: BitDepth::Int24, loops: 2, tail_seconds: 1.0 };
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
        let opts = ExportOptions { sample_rate: 48_000, bit_depth: BitDepth::Float32, loops: 1, tail_seconds: 0.5 };
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
        let opts = ExportOptions { sample_rate: 48_000, bit_depth: BitDepth::Float32, loops: 1, tail_seconds: 2.0 };
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
}
