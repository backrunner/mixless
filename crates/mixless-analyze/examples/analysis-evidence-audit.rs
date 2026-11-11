//! Verify source identity and reanalyze selected recordings in a library COPY.
//! This measures cache validity/reproducibility, not musical ground truth.
//! Usage: analysis-evidence-audit COPY.db OUTPUT_DIRECTORY [TRACK_ID ...]
use mixless_analyze::{AnalysisOptions, Analyzer, ANALYSIS_VERSION};
use mixless_library::{content_hash, Library};
use serde_json::json;
use std::{io::Write, path::Path, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err(
            "usage: analysis-evidence-audit COPY.db OUTPUT_DIRECTORY [TRACK_ID ...]".into(),
        );
    }
    let selected: Vec<i64> = args[2..]
        .iter()
        .map(|a| a.parse())
        .collect::<Result<_, _>>()?;
    let library = Library::open(Path::new(&args[0]))?;
    let output = Path::new(&args[1]);
    std::fs::create_dir_all(output)?;
    let mut log = std::fs::File::create(output.join("sources.jsonl"))?;
    let analyzer = Analyzer::new();
    let mut selected_found = vec![];
    for track in library.list_tracks()? {
        let start = Instant::now();
        let measured = content_hash(Path::new(&track.path));
        let valid_source = measured
            .as_ref()
            .is_ok_and(|hash| *hash == track.content_hash);
        let stored = library.load_analysis(track.id, ANALYSIS_VERSION)?;
        let mut result = json!({"track_id": track.id, "valid_source": valid_source,
            "source_hash": track.content_hash, "hash_seconds": start.elapsed().as_secs_f64(),
            "hash_error": measured.as_ref().err().map(ToString::to_string),
            "current_analysis": stored.is_some()});
        if selected.contains(&track.id.0) {
            selected_found.push(track.id.0);
            if !valid_source {
                return Err(format!("source identity mismatch: {}", track.id.0).into());
            }
            let before = stored.ok_or("selected track lacks a current analysis")?;
            let (mut after, report) =
                analyzer.analyze_track_with_report(track.id, Path::new(&track.path))?;
            if let Some(stems) = before.stems.clone() {
                if !Analyzer::apply_stems(&mut after, stems) {
                    return Err("incomplete stem evidence".into());
                }
            }
            if content_hash(Path::new(&track.path))? != track.content_hash {
                return Err("source changed during reanalysis".into());
            }
            let before_json = serde_json::to_value(&before)?;
            let after_json = serde_json::to_value(&after)?;
            let changed: Vec<_> = before_json
                .as_object()
                .unwrap()
                .keys()
                .filter(|k| before_json[*k] != after_json[*k])
                .collect();
            let max_beat_delta = (before.tempo.beats.len() == after.tempo.beats.len()).then(|| {
                before
                    .tempo
                    .beats
                    .iter()
                    .zip(&after.tempo.beats)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0., f32::max)
            });
            result["reanalysis"] = json!({"changed_fields": changed,
                "stored_bpm": before.tempo.global_bpm, "fresh_bpm": after.tempo.global_bpm,
                "max_beat_delta_seconds": max_beat_delta,
                "stored_key": before.key, "fresh_key": after.key,
                "native_vocal_status": format!("{:?}", report.vocal_status),
                "decode_seconds": report.decode_time.as_secs_f64(),
                "feature_seconds": report.feature_time.as_secs_f64(),
                "vocal_seconds": report.vocal_time.as_secs_f64()});
            // Transform real decoded PCM, not cached features. Disable the
            // optional classifier in BOTH runs to isolate DSP invariance.
            let dsp = Analyzer::with_options(AnalysisOptions {
                native_vocals: false,
                ..Default::default()
            });
            let buf = mixless_engine::decode_file(Path::new(&track.path))?;
            let (reference, _) = dsp.analyze_buffer(track.id, &buf);
            let prefix = (0.317 * buf.sample_rate as f32).round() as usize;
            let shift = prefix as f32 / buf.sample_rate as f32;
            let mut samples = vec![0.; prefix * 2];
            samples.extend(buf.samples.iter().map(|s| s * 0.5));
            let transformed = mixless_engine::AudioBuffer {
                frames: buf.frames + prefix as u64,
                sample_rate: buf.sample_rate,
                samples,
                loudness: Default::default(),
            };
            let (transformed, _) = dsp.analyze_buffer(track.id, &transformed);
            let mut errors: Vec<f32> = reference
                .tempo
                .beats
                .iter()
                .filter(|t| **t > 2. && **t < reference.duration_sec - 2.)
                .filter_map(|t| {
                    transformed
                        .tempo
                        .beats
                        .iter()
                        .map(|u| (*u - *t - shift).abs())
                        .min_by(f32::total_cmp)
                })
                .collect();
            errors.sort_by(f32::total_cmp);
            result["pcm_gain_and_shift"] = json!({"gain": 0.5, "shift_seconds": shift,
                "reference_bpm": reference.tempo.global_bpm, "transformed_bpm": transformed.tempo.global_bpm,
                "reference_key": reference.key, "transformed_key": transformed.key,
                "beat_p95_error_seconds": errors.get(errors.len() * 95 / 100),
                "scope": "DSP only; existing stems are not shifted or reused"});
            std::fs::write(
                output.join(format!("fresh-{}.json", track.id.0)),
                serde_json::to_vec(&after)?,
            )?;
        }
        serde_json::to_writer(&mut log, &result)?;
        writeln!(log)?;
        log.flush()?;
    }
    if selected.iter().any(|id| !selected_found.contains(id)) {
        return Err("selected track missing".into());
    }
    Ok(())
}
