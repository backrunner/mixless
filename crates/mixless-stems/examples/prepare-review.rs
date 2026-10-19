//! Build isolated four-stem listening caches; never opens or changes a library.
//! prepare-review ANALYSES.json PATHS.json MODEL_DIR CACHE_DIR
#[cfg(stems_ort)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 4 {
        return Err("Expected ANALYSES.json PATHS.json MODEL_DIR CACHE_DIR".into());
    }
    let tracks: Vec<mixless_protocol::TrackAnalysis> =
        serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let paths: Vec<String> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    if tracks.len() != paths.len() {
        return Err("Track/path count mismatch".into());
    }
    let processor = mixless_stems::Processor::new((&args[2]).into(), (&args[3]).into(), false);
    for (track, path) in tracks.iter().zip(paths) {
        let hash = mixless_library::stem_content_hash(std::path::Path::new(&path))?;
        let at = std::time::Instant::now();
        processor.analyze(
            &hash,
            track.duration_sec,
            || {
                mixless_engine::decode_file(std::path::Path::new(&path))
                    .map_err(|e| mixless_stems::Error::Model(e.to_string()))
            },
            &mut |_| {},
            &|| true,
        )?;
        let source = mixless_engine::decode_file(std::path::Path::new(&path))?;
        let playback = processor
            .playback(&hash, source.frames, source.sample_rate)?
            .ok_or("Generated cache cannot be loaded for playback")?;
        if !playback.has_bass() {
            return Err("Generated cache has no independent bass".into());
        }
        println!(
            "{} PCM playback ready at {} Hz ({} bytes)",
            track.track_id.0,
            source.sample_rate,
            playback.bytes()
        );
        println!(
            "{} four-stem cache ready in {:.1}s",
            track.track_id.0,
            at.elapsed().as_secs_f32()
        );
    }
    Ok(())
}
#[cfg(not(stems_ort))]
fn main() {
    eprintln!("Four-stem inference unavailable on this platform");
    std::process::exit(1);
}
