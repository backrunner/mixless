//! Re-run basic analysis for every track in a COPY of a library database and
//! store the result under a new analysis version. Stem evidence from the old
//! version is re-attached without inference; the live library is never used.
//! Usage: reanalyze-copy LIBRARY-COPY.db OLD_VERSION NEW_VERSION
use mixless_analyze::Analyzer;
use mixless_library::Library;
use std::path::Path;
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let (Some(db), Some(old), Some(new)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("usage: reanalyze-copy LIBRARY-COPY.db OLD_VERSION NEW_VERSION");
        std::process::exit(2);
    };
    let old: u32 = old.parse()?;
    let new: u32 = new.parse()?;
    let library = Library::open(Path::new(db))?;
    let analyzer = Analyzer::new();
    let start = Instant::now();
    let mut done = 0usize;
    for track in library.list_tracks()? {
        if !Path::new(&track.path).exists() {
            println!("{}: missing file, skipped", track.path);
            continue;
        }
        let previous = library.load_analysis(track.id, old)?;
        let mut analysis = match analyzer.analyze_track(track.id, Path::new(&track.path)) {
            Ok(a) => a,
            Err(e) => {
                println!("{} {}: analyze failed: {e}", track.id.0, track.title);
                continue;
            }
        };
        if let Some(stems) = previous.and_then(|a| a.stems) {
            if !Analyzer::apply_stems(&mut analysis, stems) {
                println!(
                    "{} {}: stem evidence did not cover the track",
                    track.id.0, track.title
                );
            }
        }
        library.save_analysis(&analysis, &track.content_hash, new)?;
        let mut pulse = analysis.tempo.pulse_confidence.clone();
        pulse.sort_by(f32::total_cmp);
        let min = pulse.first().copied().unwrap_or(0.);
        let median = pulse.get(pulse.len() / 2).copied().unwrap_or(0.);
        println!(
            "{} {}: bpm={:.1} segments={} pulse min={:.2} median={:.2}",
            track.id.0,
            track.title,
            analysis.tempo.global_bpm,
            analysis.tempo.segments.len(),
            min,
            median
        );
        done += 1;
    }
    println!("reanalyzed {done} tracks in {:.1?}", start.elapsed());
    Ok(())
}
