//! Exercise production tag writeback on new copies, never on source recordings.
//! tag-writeback-audit INPUT.json NEW_OUTPUT_DIRECTORY
//! Input rows: { "path": "...", "bpm": 150.0, "key": "A major", "camelot": "11B" }
use mixless_library::{stem_content_hash, Library};
use serde::Deserialize;
use serde_json::json;
use std::{error::Error, path::PathBuf};

#[derive(Deserialize)]
struct Input {
    path: PathBuf,
    bpm: f32,
    key: String,
    camelot: String,
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("Expected INPUT.json NEW_OUTPUT_DIRECTORY".into());
    }
    let inputs: Vec<Input> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let root = PathBuf::from(&args[1]);
    std::fs::create_dir(&root)?;
    let audio = root.join("audio");
    std::fs::create_dir(&audio)?;
    let library = Library::open(&root.join("library.db"))?;
    let mut results = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let source_before = blake3::hash(&std::fs::read(&input.path)?);
        let identity = stem_content_hash(&input.path)?;
        let extension = input
            .path
            .extension()
            .and_then(|x| x.to_str())
            .ok_or("Missing extension")?;
        let path = audio.join(format!("{:02}.{extension}", index + 1));
        std::fs::copy(&input.path, &path)?;
        let id = library.import_file(&path)?;
        library.set_analysis_meta(id, Some(input.bpm), Some(&input.key), Some(&input.camelot))?;
        let written = library.write_track_metadata(id)?;
        let after = library.get_track(id)?.content_hash;
        let unchanged_audio = identity == stem_content_hash(&path)?;
        let idempotent = !library.write_track_metadata(id)?;
        let unchanged_source = source_before == blake3::hash(&std::fs::read(&input.path)?);
        if !unchanged_audio || !idempotent || !unchanged_source {
            return Err(format!("Tag integrity check failed for {}", input.path.display()).into());
        }
        results.push(json!({
            "source": input.path, "copy": path, "bpm": input.bpm,
            "written": written, "source_blake3": source_before.to_hex().to_string(),
            "copy_blake3": after, "audio_identity": identity,
            "unchanged_audio": unchanged_audio, "unchanged_source": unchanged_source,
            "idempotent": idempotent,
        }));
    }
    std::fs::write(
        root.join("results.json"),
        serde_json::to_vec_pretty(&results)?,
    )?;
    println!("{} copies verified", results.len());
    Ok(())
}
