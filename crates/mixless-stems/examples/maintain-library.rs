//! Explicit local-library maintenance. Source audio is never re-encoded.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::path::{Path, PathBuf};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[1] != "--apply" {
        return Err(
            "Usage: maintain-library DATA_DIR --apply (back up the library and source files first)"
                .into(),
        );
    }
    let root = PathBuf::from(&args[0]);
    let library = mixless_library::Library::open(&root.join("library.db"))?;
    let mut written = 0;
    let mut failed = 0;
    for track in library.list_tracks()? {
        if !Path::new(&track.path).is_file() {
            continue;
        }
        match library.write_track_metadata(track.id) {
            Ok(changed) => written += usize::from(changed),
            Err(error) => {
                failed += 1;
                eprintln!("track {}: {error}", track.id.0);
            }
        }
    }
    let hashes: Vec<_> = library
        .stem_cache_keys(None)?
        .into_iter()
        .map(|(_, hash)| hash)
        .collect();
    let processor =
        mixless_stems::Processor::new(root.join("models"), root.join("stem-cache"), false);
    let freed = processor.prune_cache(&hashes)?;
    println!(
        "{{\"tags_written\":{written},\"tags_failed\":{failed},\"cache_bytes_freed\":{freed}}}"
    );
    if failed > 0 {
        return Err("Some metadata could not be written; see track errors".into());
    }
    Ok(())
}
