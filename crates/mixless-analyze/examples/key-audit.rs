//! Device-free whole-track and local key evidence; never writes the music library.
//! cargo run --release -p mixless-analyze --example key-audit -- OUTPUT.json TRACK...
//! Local windows use high-resolution tonal observations on the source clock.
//! These are diagnostic estimates, not annotated musical keys.
use mixless_analyze::{AnalysisOptions, Analyzer, ANALYSIS_VERSION};
use mixless_protocol::{local_tonality, tonal_profile, tonal_windows, TrackAnalysis, TrackId};
use serde_json::{json, Value};
use std::{error::Error, io::Read, path::Path};

fn local(track: &TrackAnalysis, start: f32, end: f32) -> Value {
    let profile = tonal_profile(track, start, end);
    let (key, camelot, confidence) = local_tonality(track, start, end);
    json!({
        "start_sec": start, "end_sec": end, "covered_sec": profile.coverage * (end-start),
        "key": key, "camelot": camelot, "confidence": confidence,
        "harmonic_confidence": profile.harmonic_confidence, "chroma": profile.chroma,
        "candidates": profile.candidates, "contexts": tonal_windows(track,start,end),
    })
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let output = args.next().ok_or("Expected OUTPUT.json TRACK...")?;
    let paths: Vec<_> = args.collect();
    if paths.is_empty() {
        return Err("Expected at least one audio path".into());
    }
    let analyzer = Analyzer::with_options(AnalysisOptions {
        native_vocals: false,
        ..Default::default()
    });
    let mut results = Vec::new();
    let mut analyses = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let path = Path::new(path);
        let mut source = std::fs::File::open(path)?;
        let mut hash = blake3::Hasher::new();
        let mut buffer = [0; 65536];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let (track, timing) = analyzer.analyze_track_with_report(TrackId(index as i64), path)?;
        let windows: Vec<_> = (0..(track.duration_sec / 16.).ceil() as usize)
            .map(|i| {
                local(
                    &track,
                    i as f32 * 16.,
                    ((i + 1) as f32 * 16.).min(track.duration_sec),
                )
            })
            .collect();
        let sections: Vec<_> = track
            .sections
            .iter()
            .map(|section| {
                let mut result = local(&track, section.start_sec, section.end_sec);
                result["detected_section"] = json!(section.label);
                result
            })
            .collect();
        results.push(json!({
            "path": path, "blake3": hash.finalize().to_hex().to_string(),
            "duration_sec": track.duration_sec, "sample_rate": track.sample_rate,
            "key": track.key, "camelot": track.camelot, "confidence": track.key_confidence,
            "bpm": track.tempo.global_bpm, "windows": windows, "sections": sections,
        }));
        eprintln!(
            "{}/{}: {} ({:.2}s)",
            index + 1,
            paths.len(),
            path.display(),
            (timing.decode_time + timing.feature_time).as_secs_f64()
        );
        analyses.push(track);
        if let Ok(path) = std::env::var("AUDIT_ANALYSES_OUT") {
            std::fs::write(path, serde_json::to_vec(&analyses)?)?;
        }
        // Keep completed work if a later file fails; completion is explicit.
        std::fs::write(
            &output,
            serde_json::to_vec_pretty(&json!({
                "analysis_version": ANALYSIS_VERSION, "native_vocals": false,
                "requested_tracks": paths.len(), "complete": results.len() == paths.len(),
                "tracks": results,
            }))?,
        )?;
    }
    Ok(())
}
