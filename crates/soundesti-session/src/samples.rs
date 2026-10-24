//! Audio file decoding for sample-based instruments (WAV: 8/16/24/32-bit int and 32-bit float,
//! mono or multichannel). Runs on the control thread, never on the audio thread.

use std::path::Path;
use std::sync::Arc;

use soundesti_plugin_api::SampleData;

/// Largest sample accepted (10 minutes at 48 kHz) so a wrong file can't exhaust memory.
const MAX_FRAMES: usize = 48_000 * 600;

pub fn load_wav(path: &Path) -> Result<Arc<SampleData>, String> {
    let mut reader = hound::WavReader::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let spec = reader.spec();
    let channels = spec.channels.max(1) as usize;
    let frames = reader.duration() as usize;
    if frames > MAX_FRAMES {
        return Err(format!("{} is too long ({} s)", path.display(), frames / spec.sample_rate.max(1) as usize));
    }
    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader.samples::<f32>().collect::<Result<_, _>>().map_err(|e| e.to_string())?,
        hound::SampleFormat::Int => {
            let scale = 1.0 / (1u64 << (spec.bits_per_sample.clamp(8, 32) - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|s| s.map(|v| v as f32 * scale))
                .collect::<Result<_, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    let n = samples.len() / channels;
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for f in samples.chunks_exact(channels) {
        let l = sanitize(f[0]);
        left.push(l);
        right.push(if channels > 1 { sanitize(f[1]) } else { l });
    }
    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Arc::new(SampleData { left, right, sample_rate: spec.sample_rate as f32, name }))
}

fn sanitize(x: f32) -> f32 {
    if x.is_finite() { x.clamp(-4.0, 4.0) } else { 0.0 }
}

pub fn is_supported(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, spec: hound::WavSpec, frames: &[[f32; 2]]) {
        let mut w = hound::WavWriter::create(path, spec).unwrap();
        for f in frames {
            for &x in &f[..spec.channels as usize] {
                match spec.sample_format {
                    hound::SampleFormat::Float => w.write_sample(x).unwrap(),
                    hound::SampleFormat::Int => w.write_sample((x * 32767.0) as i16).unwrap(),
                }
            }
        }
        w.finalize().unwrap();
    }

    #[test]
    fn loads_mono_int_and_stereo_float() {
        let dir = std::env::temp_dir().join(format!("sd-samples-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mono = dir.join("mono.wav");
        write(
            &mono,
            hound::WavSpec {
                channels: 1,
                sample_rate: 44_100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
            &[[0.5, 0.0], [-0.5, 0.0]],
        );
        let s = load_wav(&mono).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s.sample_rate, 44_100.0);
        assert!((s.left[0] - 0.5).abs() < 1e-3 && s.left == s.right, "mono is duplicated to both sides");
        let st = dir.join("stereo.wav");
        write(
            &st,
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
            &[[0.25, -0.75]],
        );
        let s = load_wav(&st).unwrap();
        assert_eq!((s.left[0], s.right[0]), (0.25, -0.75));
        assert_eq!(s.name, "stereo");
        assert!(load_wav(&dir.join("missing.wav")).is_err());
        std::fs::write(dir.join("junk.wav"), b"not a wav").unwrap();
        assert!(load_wav(&dir.join("junk.wav")).is_err());
        std::fs::remove_dir_all(dir).ok();
    }
}
