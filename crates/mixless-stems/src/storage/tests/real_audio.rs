use super::*;
use std::time::Instant;

#[test]
#[ignore = "Read-only audit of existing real stem caches; writes to MIXLESS_STORAGE_REPORT"]
fn audit_real_caches() {
    let source = std::env::var_os("MIXLESS_STEM_CACHE").expect("MIXLESS_STEM_CACHE");
    let output = std::env::var_os("MIXLESS_STORAGE_REPORT").expect("MIXLESS_STORAGE_REPORT");
    let output = Path::new(&output);
    fs::create_dir_all(output).unwrap();
    let mut candidates: Vec<_> = fs::read_dir(source)
        .unwrap()
        .flatten()
        .filter_map(|e| {
            let manifest = cache::manifest(&e.path()).ok()?;
            (crate::is_current(&manifest.analysis)
                && manifest.storage.is_none()
                && manifest.analysis.duration_sec < 360.
                && e.path().join("bass.wav").exists())
            .then_some((manifest.analysis.duration_sec, e.path()))
        })
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert!(candidates.len() >= 3);
    let mut report = Vec::new();
    let selected: Vec<usize> = std::env::var("MIXLESS_AUDIT_INDEX")
        .ok()
        .map(|s| vec![s.parse().unwrap()])
        .unwrap_or_else(|| vec![0, candidates.len() / 2, candidates.len() - 1]);
    for &index in &selected {
        let (duration, source) = &candidates[index];
        let manifest = cache::manifest(source).unwrap();
        let mut lanes: Vec<_> = (0..4)
            .map(|i| cache::audio(source, &manifest, i, &|| true).unwrap())
            .collect();
        let bass = lanes.pop().unwrap();
        let stems = Stems {
            audio: lanes.try_into().unwrap(),
            bass,
            residual_rms: manifest.analysis.residual_rms,
        };
        for format in [StorageFormat::Wav, StorageFormat::Flac, StorageFormat::Mp3] {
            let key = format!("{index}-{format:?}");
            let p = Processor::new(output.join("no-models"), output.join("cache"), false);
            let target = p.cache_path(&key);
            let start = Instant::now();
            cache::save_as(&target, &stems, &manifest.analysis, format, &|| true).unwrap();
            let save_sec = start.elapsed().as_secs_f64();
            let encoded = cache::manifest(&target).unwrap();
            let storage = encoded.storage.as_ref().unwrap();
            let start = Instant::now();
            let loaded = p.playback(&key, storage.frames, RATE).unwrap().unwrap();
            let load_sec = start.elapsed().as_secs_f64();
            assert_eq!(loaded.bytes() as u64, storage.frames * 24);
            drop(loaded);
            let mut errors = Vec::new();
            for i in 0..4 {
                let decoded = cache::audio(&target, &encoded, i, &|| true).unwrap();
                let raw = if i == 3 { &stems.bass } else { &stems.audio[i] };
                assert_eq!(raw.len(), decoded.len());
                let mut peak = 0f32;
                let mut noise = 0f64;
                let mut signal = 0f64;
                for (&a, &b) in raw.iter().zip(&decoded) {
                    peak = peak.max((a - b).abs());
                    noise += (a as f64 - b as f64).powi(2);
                    signal += (a as f64).powi(2);
                }
                if format == StorageFormat::Wav {
                    assert_eq!(peak, 0.);
                }
                if format == StorageFormat::Flac {
                    assert!(peak <= storage.gain[i] / 16777216.);
                }
                errors.push(serde_json::json!({"stem": NAMES[i], "peak_error": peak, "snr_db": if noise > 0. {Some(10. * (signal / noise).log10())} else {None}, "gain": storage.gain[i]}));
            }
            let entry = serde_json::json!({"source_cache": source.file_name().unwrap().to_string_lossy(), "duration_sec": duration, "format": format, "bytes": storage.bytes.iter().sum::<u64>(), "raw_bytes": storage.frames * 32, "save_verify_sec": save_sec, "playback_load_sec": load_sec, "errors": errors});
            eprintln!("{entry}");
            report.push(entry);
            fs::write(
                output.join("report.json"),
                serde_json::to_vec_pretty(&report).unwrap(),
            )
            .unwrap();
            // Personal audio is only used transiently; keep measurements, not copies.
            fs::remove_dir_all(target).unwrap();
        }
    }
}
