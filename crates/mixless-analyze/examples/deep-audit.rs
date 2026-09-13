//! Upgrade audited source tracks using the same native inference as the desktop.
use std::{io::Read, path::Path, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err("deep-audit ANALYSES PATHS MODEL_DIR CACHE_DIR OUTPUT".into());
    }
    let mut tracks: Vec<mixless_protocol::TrackAnalysis> =
        serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let paths: Vec<String> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    if tracks.len() != paths.len() {
        return Err("Mismatched track count".into());
    }
    let processor = mixless_stems::Processor::new((&args[2]).into(), (&args[3]).into(), true);
    for (track, path) in tracks.iter_mut().zip(paths) {
        let start = Instant::now();
        let mut hash = blake3::Hasher::new();
        let mut input = std::fs::File::open(&path)?;
        let mut buf = [0; 65536];
        loop {
            let n = input.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
        }
        let hash = hash.finalize().to_hex().to_string();
        let stems = processor.analyze(
            &hash,
            track.duration_sec,
            || {
                mixless_engine::decode_file(Path::new(&path))
                    .map_err(|e| mixless_stems::Error::Model(e.to_string()))
            },
            &mut |_| {},
            &|| true,
        )?;
        let note_count = stems.notes.len();
        if !mixless_analyze::Analyzer::apply_stems(track, stems) {
            return Err("Evidence did not cover source audio".into());
        }
        eprintln!(
            "Track {}: {note_count} notes, {:.2}s, {}",
            track.track_id.0,
            start.elapsed().as_secs_f64(),
            processor.cache_path(&hash).display()
        );
    }
    std::fs::write(&args[4], serde_json::to_vec(&tracks)?)?;
    Ok(())
}
