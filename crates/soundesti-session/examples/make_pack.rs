//! Renders the bundled "Soundesti Essentials" sample pack from Soundesti's own instruments
//! and effects (so every sound is original and freely redistributable under the project
//! license). Run: `scripts\cargo.ps1 run -p soundesti-session --example make_pack`
//! Writes WAV files to `assets/packs/Soundesti Essentials/`.

use std::path::{Path, PathBuf};

use soundesti_plugin_api::{NoteEvent, TimedEvent};

const SR: u32 = 48_000;
const BLOCK: usize = 512;

struct Sound {
    folder: &'static str,
    name: &'static str,
    instrument: &'static str,
    params: &'static [(&'static str, f32)],
    key: u8,
    /// Seconds the note is held.
    gate: f32,
    /// Maximum rendered length in seconds (trailing silence is trimmed).
    max: f32,
    /// Insert effects applied after the instrument: (type id, params).
    fx: &'static [(&'static str, &'static [(&'static str, f32)])],
}

const VERB: &str = "soundesti.reverb";
const DIST: &str = "soundesti.distortion";
const DELAY: &str = "soundesti.delay";
const FILTER: &str = "soundesti.filter";

#[rustfmt::skip]
const SOUNDS: &[Sound] = &[
    // Kicks
    Sound { folder: "Kicks", name: "SE Kick Punch", instrument: "soundesti.kickr", params: &[("pitch.start", 220.0), ("pitch.end", 50.0), ("pitch.decay", 0.045), ("amp.decay", 0.4), ("amp.click", 0.5), ("amp.drive", 0.35)], key: 60, gate: 0.1, max: 1.0, fx: &[] },
    Sound { folder: "Kicks", name: "SE Kick Deep", instrument: "soundesti.kickr", params: &[("pitch.start", 150.0), ("pitch.end", 40.0), ("pitch.decay", 0.08), ("amp.decay", 0.9), ("amp.click", 0.2), ("amp.drive", 0.15)], key: 60, gate: 0.1, max: 1.5, fx: &[] },
    Sound { folder: "Kicks", name: "SE Kick Tight", instrument: "soundesti.kickr", params: &[("pitch.start", 300.0), ("pitch.end", 60.0), ("pitch.decay", 0.025), ("amp.decay", 0.18), ("amp.click", 0.7), ("amp.drive", 0.4)], key: 60, gate: 0.1, max: 0.6, fx: &[] },
    Sound { folder: "Kicks", name: "SE Kick Distorted", instrument: "soundesti.kickr", params: &[("pitch.start", 260.0), ("pitch.end", 45.0), ("pitch.decay", 0.06), ("amp.decay", 0.6), ("amp.click", 0.4), ("amp.drive", 0.9)], key: 60, gate: 0.1, max: 1.2, fx: &[(DIST, &[("dist.drive", 18.0), ("dist.tone", 6_000.0), ("dist.output", -8.0)])] },
    Sound { folder: "Kicks", name: "SE Kick Boom 808", instrument: "soundesti.kickr", params: &[("pitch.start", 120.0), ("pitch.end", 44.0), ("pitch.decay", 0.12), ("amp.decay", 1.8), ("amp.click", 0.15), ("amp.drive", 0.25)], key: 60, gate: 0.1, max: 2.5, fx: &[] },
    Sound { folder: "Kicks", name: "SE Kick Soft", instrument: "soundesti.kickr", params: &[("pitch.start", 110.0), ("pitch.end", 50.0), ("pitch.decay", 0.05), ("amp.decay", 0.35), ("amp.click", 0.0), ("amp.drive", 0.0)], key: 60, gate: 0.1, max: 0.8, fx: &[] },
    // Claps
    Sound { folder: "Claps", name: "SE Clap Classic", instrument: "soundesti.clapper", params: &[("mode.claps", 3.0), ("noise.tone", 2_200.0), ("amp.decay", 0.2)], key: 60, gate: 0.1, max: 0.8, fx: &[] },
    Sound { folder: "Claps", name: "SE Clap Tight", instrument: "soundesti.clapper", params: &[("mode.claps", 2.0), ("noise.tone", 3_500.0), ("amp.decay", 0.1)], key: 60, gate: 0.1, max: 0.5, fx: &[] },
    Sound { folder: "Claps", name: "SE Clap Wide Room", instrument: "soundesti.clapper", params: &[("mode.claps", 4.0), ("noise.tone", 1_800.0), ("amp.decay", 0.25)], key: 60, gate: 0.1, max: 2.0, fx: &[(VERB, &[("reverb.size", 0.7), ("reverb.mix", 0.35)])] },
    Sound { folder: "Claps", name: "SE Clap Snap", instrument: "soundesti.clapper", params: &[("mode.claps", 1.0), ("noise.tone", 5_000.0), ("amp.decay", 0.08)], key: 60, gate: 0.1, max: 0.4, fx: &[] },
    // Snares
    Sound { folder: "Snares", name: "SE Snare Crack", instrument: "soundesti.clapper", params: &[("mode.claps", 0.0), ("body.tone", 210.0), ("body.mix", 0.5), ("noise.tone", 5_000.0), ("noise.snappy", 0.9), ("amp.decay", 0.18)], key: 60, gate: 0.1, max: 0.8, fx: &[] },
    Sound { folder: "Snares", name: "SE Snare Fat", instrument: "soundesti.clapper", params: &[("mode.claps", 0.0), ("body.tone", 160.0), ("body.mix", 0.7), ("noise.tone", 3_000.0), ("noise.snappy", 0.6), ("amp.decay", 0.3)], key: 60, gate: 0.1, max: 1.0, fx: &[] },
    Sound { folder: "Snares", name: "SE Snare Rim", instrument: "soundesti.clapper", params: &[("mode.claps", 0.0), ("body.tone", 380.0), ("body.mix", 0.9), ("noise.tone", 9_000.0), ("noise.snappy", 0.2), ("amp.decay", 0.05)], key: 60, gate: 0.1, max: 0.3, fx: &[] },
    Sound { folder: "Snares", name: "SE Snare Hall", instrument: "soundesti.clapper", params: &[("mode.claps", 0.0), ("body.tone", 190.0), ("body.mix", 0.45), ("noise.tone", 4_200.0), ("noise.snappy", 0.8), ("amp.decay", 0.2)], key: 60, gate: 0.1, max: 2.5, fx: &[(VERB, &[("reverb.size", 0.85), ("reverb.mix", 0.4)])] },
    // Hats
    Sound { folder: "Hats", name: "SE Hat Closed", instrument: "soundesti.hatz", params: &[("tone.cutoff", 8_000.0), ("amp.decay", 0.05)], key: 60, gate: 0.05, max: 0.3, fx: &[] },
    Sound { folder: "Hats", name: "SE Hat Closed Bright", instrument: "soundesti.hatz", params: &[("tone.cutoff", 12_000.0), ("amp.decay", 0.04), ("tone.metal", 0.85)], key: 60, gate: 0.05, max: 0.3, fx: &[] },
    Sound { folder: "Hats", name: "SE Hat Closed Soft", instrument: "soundesti.hatz", params: &[("tone.cutoff", 5_000.0), ("amp.decay", 0.06), ("tone.metal", 0.3)], key: 60, gate: 0.05, max: 0.3, fx: &[] },
    Sound { folder: "Hats", name: "SE Hat Open", instrument: "soundesti.hatz", params: &[("tone.cutoff", 8_000.0), ("amp.decay", 0.45)], key: 60, gate: 0.4, max: 1.2, fx: &[] },
    Sound { folder: "Hats", name: "SE Hat Open Long", instrument: "soundesti.hatz", params: &[("tone.cutoff", 10_000.0), ("amp.decay", 0.9), ("tone.metal", 0.8)], key: 60, gate: 0.8, max: 2.0, fx: &[] },
    Sound { folder: "Hats", name: "SE Ride Shimmer", instrument: "soundesti.hatz", params: &[("tone.cutoff", 14_000.0), ("amp.decay", 1.4), ("tone.metal", 1.0)], key: 60, gate: 1.2, max: 3.0, fx: &[(VERB, &[("reverb.size", 0.6), ("reverb.mix", 0.2)])] },
    // Bass (C2 = key 36)
    Sound { folder: "Bass", name: "SE Bass Saw C2", instrument: "soundesti.trident", params: &[("filter.cutoff", 900.0), ("filter.resonance", 2.0), ("amp.decay", 0.4), ("amp.sustain", 0.5)], key: 36, gate: 0.6, max: 1.5, fx: &[] },
    Sound { folder: "Bass", name: "SE Bass Sub C2", instrument: "soundesti.trident", params: &[("filter.cutoff", 200.0), ("filter.resonance", 0.7), ("amp.sustain", 0.9)], key: 36, gate: 0.8, max: 1.5, fx: &[] },
    Sound { folder: "Bass", name: "SE Bass FM Growl C2", instrument: "soundesti.orbit", params: &[("mod.ratio", 1.0), ("mod.index", 6.0), ("mod.decay", 0.4), ("mod.feedback", 0.4), ("amp.sustain", 0.7)], key: 36, gate: 0.6, max: 1.5, fx: &[(FILTER, &[])] },
    // Plucks and keys (C4 = key 60)
    Sound { folder: "Plucks", name: "SE Pluck Bright C4", instrument: "soundesti.harpa", params: &[("string.brightness", 0.85), ("string.sustain", 1.5)], key: 60, gate: 0.5, max: 2.5, fx: &[] },
    Sound { folder: "Plucks", name: "SE Pluck Muted C4", instrument: "soundesti.harpa", params: &[("string.brightness", 0.25), ("string.sustain", 0.5), ("string.pick", 0.4)], key: 60, gate: 0.3, max: 1.2, fx: &[] },
    Sound { folder: "Plucks", name: "SE Pluck Echo C4", instrument: "soundesti.harpa", params: &[("string.brightness", 0.6), ("string.sustain", 1.0)], key: 60, gate: 0.3, max: 3.0, fx: &[(DELAY, &[("delay.sync", 0.0), ("delay.time", 250.0), ("delay.feedback", 0.45), ("delay.mix", 0.4)])] },
    Sound { folder: "Keys", name: "SE Bell FM C4", instrument: "soundesti.orbit", params: &[("mod.ratio", 3.5), ("mod.index", 4.0), ("mod.decay", 1.5), ("amp.decay", 2.0), ("amp.sustain", 0.0)], key: 60, gate: 1.0, max: 3.5, fx: &[(VERB, &[("reverb.size", 0.6), ("reverb.mix", 0.25)])] },
    Sound { folder: "Keys", name: "SE E-Piano FM C4", instrument: "soundesti.orbit", params: &[("mod.ratio", 1.0), ("mod.index", 1.8), ("mod.decay", 0.8), ("amp.decay", 1.5), ("amp.sustain", 0.2)], key: 60, gate: 1.0, max: 3.0, fx: &[] },
    Sound { folder: "Keys", name: "SE Lead Saw C4", instrument: "soundesti.trident", params: &[("filter.cutoff", 4_000.0), ("filter.resonance", 1.5), ("amp.sustain", 0.8)], key: 60, gate: 1.0, max: 2.0, fx: &[(DELAY, &[("delay.sync", 0.0), ("delay.time", 300.0), ("delay.mix", 0.25)])] },
    // FX
    Sound { folder: "FX", name: "SE FX Impact", instrument: "soundesti.kickr", params: &[("pitch.start", 400.0), ("pitch.end", 30.0), ("pitch.decay", 0.3), ("amp.decay", 1.5), ("amp.drive", 0.8)], key: 60, gate: 0.1, max: 5.0, fx: &[(VERB, &[("reverb.size", 1.0), ("reverb.mix", 0.5), ("reverb.damping", 0.2)])] },
    Sound { folder: "FX", name: "SE FX Noise Burst", instrument: "soundesti.clapper", params: &[("mode.claps", 0.0), ("body.mix", 0.0), ("noise.tone", 6_000.0), ("amp.decay", 0.6)], key: 60, gate: 0.1, max: 4.0, fx: &[(VERB, &[("reverb.size", 0.9), ("reverb.mix", 0.6)])] },
    Sound { folder: "FX", name: "SE FX Crushed Zap", instrument: "soundesti.kickr", params: &[("pitch.start", 1_000.0), ("pitch.end", 100.0), ("pitch.decay", 0.15), ("amp.decay", 0.3)], key: 60, gate: 0.1, max: 1.0, fx: &[(DIST, &[("dist.mode", 3.0), ("dist.drive", 36.0), ("dist.output", -4.0)])] },
];

fn main() {
    let out = root().join("assets").join("packs").join("Soundesti Essentials");
    for s in SOUNDS {
        let (mut l, mut r) = render(s);
        normalize_and_trim(&mut l, &mut r);
        let dir = out.join(s.folder);
        std::fs::create_dir_all(&dir).expect("create pack folder");
        let path = dir.join(format!("{}.wav", s.name));
        write_wav(&path, &l, &r);
        println!("{:>6.2} s  {}", l.len() as f32 / SR as f32, path.display());
    }
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn render(s: &Sound) -> (Vec<f32>, Vec<f32>) {
    let mut inst = soundesti_synthesis::create_instrument(s.instrument, SR as f32).expect("known instrument");
    let infos = soundesti_synthesis::params_for(s.instrument);
    for (k, v) in s.params {
        let p = infos.iter().find(|p| p.key == *k).unwrap_or_else(|| panic!("{}: unknown param {k}", s.name));
        inst.set_param(p.id, *v);
    }
    let mut fx: Vec<_> = s
        .fx
        .iter()
        .map(|(id, params)| {
            let mut e = soundesti_effects::create(id, SR as f32).expect("known effect");
            let infos = soundesti_effects::params_for(id);
            for (k, v) in *params {
                let p =
                    infos.iter().find(|p| p.key == *k).unwrap_or_else(|| panic!("{}: unknown fx param {k}", s.name));
                e.set_param(p.id, *v);
            }
            e
        })
        .collect();
    let total = (s.max * SR as f32) as usize;
    let gate = (s.gate * SR as f32) as usize;
    let (mut l, mut r) = (vec![0.0f32; total], vec![0.0f32; total]);
    let mut start = 0;
    while start < total {
        let n = BLOCK.min(total - start);
        let mut events = Vec::new();
        if start == 0 {
            events.push(TimedEvent { offset: 0, event: NoteEvent::On { id: 1, key: s.key, velocity: 0.9 } });
        }
        if (start..start + n).contains(&gate) {
            events.push(TimedEvent { offset: (gate - start) as u32, event: NoteEvent::Off { id: 1 } });
        }
        let (bl, br) = (&mut l[start..start + n], &mut r[start..start + n]);
        inst.process(&events, bl, br);
        for e in &mut fx {
            e.process(bl, br);
        }
        start += n;
    }
    (l, r)
}

/// Peak-normalizes to -1 dBFS and trims trailing silence with a short fade.
fn normalize_and_trim(l: &mut Vec<f32>, r: &mut Vec<f32>) {
    let peak = l.iter().chain(r.iter()).fold(0.0f32, |m, x| m.max(x.abs()));
    assert!(peak > 1e-4, "silent render");
    let g = 0.89 / peak;
    l.iter_mut().chain(r.iter_mut()).for_each(|x| *x *= g);
    let last = (0..l.len()).rev().find(|&i| l[i].abs().max(r[i].abs()) > 3e-4).unwrap_or(0);
    // A tail that reaches the length cap gets a long fade so it doesn't cut off audibly.
    let cut = last + SR as usize / 100 >= l.len();
    let fade = (if cut { SR as usize / 4 } else { SR as usize / 100 }).min(last);
    let end = (last + fade).min(l.len());
    l.truncate(end);
    r.truncate(end);
    for i in 0..fade.min(end) {
        let k = i as f32 / fade as f32;
        let j = end - 1 - i;
        l[j] *= k;
        r[j] *= k;
    }
}

fn write_wav(path: &Path, l: &[f32], r: &[f32]) {
    let spec =
        hound::WavSpec { channels: 2, sample_rate: SR, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    for (a, b) in l.iter().zip(r) {
        w.write_sample((a.clamp(-1.0, 1.0) * 32_767.0) as i16).unwrap();
        w.write_sample((b.clamp(-1.0, 1.0) * 32_767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}
