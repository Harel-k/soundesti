//! Imports .flp files and prints a summary of what came through.
//! Usage: `cargo run -p soundesti-flp --example inspect -- <file.flp>...`

fn main() {
    let mut failures = 0;
    for arg in std::env::args().skip(1).filter(|a| a.to_lowercase().ends_with(".flp")) {
        let path = std::path::Path::new(&arg);
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        match soundesti_flp::import_file(path) {
            Ok(imp) => {
                let p = &imp.project;
                let notes: usize = p.patterns.iter().flat_map(|pt| pt.channels.iter()).map(|c| c.notes.len()).sum();
                let samplers = p.channels.iter().filter(|c| c.instrument.type_id == "soundesti.sampler").count();
                let found = p
                    .channels
                    .iter()
                    .filter(|c| c.instrument.sample.as_ref().is_some_and(|s| std::path::Path::new(s).is_file()))
                    .count();
                println!(
                    "OK   {name}: FL {} | {:.1} BPM {}/{} | {} channels ({samplers} samplers, {found} samples found) | {} patterns, {notes} notes | {} clips, song length {} bars",
                    imp.fl_version,
                    p.bpm,
                    p.time_signature.numerator,
                    p.time_signature.denominator,
                    p.channels.len(),
                    p.patterns.len(),
                    p.playlist.clips.len(),
                    p.playlist.song_length(p.time_signature) / p.time_signature.ticks_per_bar(),
                );
                for r in imp.report.iter().take(4) {
                    println!("       - {r}");
                }
                if imp.report.len() > 4 {
                    println!("       - ... {} more", imp.report.len() - 4);
                }
            }
            Err(e) => {
                failures += 1;
                println!("FAIL {name}: {e}");
            }
        }
    }
    std::process::exit(if failures > 0 { 1 } else { 0 });
}
