use super::*;
use crate::{Processor, Stems, cache};
use std::{fs, path::Path, sync::Arc};

mod real_audio;
mod render;

fn fixture(frames: usize) -> Stems {
    let lane = |frequency: f32| {
        (0..frames)
            .flat_map(|i| {
                let s = 0.12 * (std::f32::consts::TAU * frequency * i as f32 / RATE as f32).sin();
                [s, s * 0.7]
            })
            .collect()
    };
    Stems {
        audio: [lane(331.), lane(127.), lane(719.)],
        bass: lane(53.),
        residual_rms: 0.,
    }
}

#[test]
fn cache_modes_play_resample_transfer_and_reject_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let p = Processor::new(dir.path().join("models"), dir.path().join("cache"), false);
    let stems = fixture(44100 * 3 + 317);
    let analysis = crate::evidence::extract(&stems, vec![]);
    for format in [StorageFormat::Wav, StorageFormat::Flac, StorageFormat::Mp3] {
        let hash = format!("{format:?}");
        let path = p.cache_path(&hash);
        cache::save_as(&path, &stems, &analysis, format, &|| true).unwrap();
        let manifest = cache::manifest(&path).unwrap();
        let frames = manifest.storage.as_ref().unwrap().frames;
        for rate in [44100, 48000] {
            let n = (frames as f64 * rate as f64 / RATE as f64).round() as u64;
            let loaded = p.playback(&hash, n, rate).unwrap().unwrap();
            assert_eq!(loaded.bytes(), n as usize * 24);
            assert!(loaded.has_bass());
        }
        let imported = Processor::new(
            dir.path().join("models"),
            dir.path().join("imported"),
            false,
        );
        imported
            .install_portable_cache(&hash, analysis.duration_sec, &path, &|| true)
            .unwrap();
        assert!(imported.playback(&hash, frames, RATE).unwrap().is_some());
        p.set_storage_format(StorageFormat::Mp3);
        p.analyze(
            &hash,
            analysis.duration_sec,
            || panic!("existing caches must be reused"),
            &mut |_| {},
            &|| true,
        )
        .unwrap();
        assert_eq!(cache::manifest(&path).unwrap().format(), format);
        // Cancellation cannot replace a published entry or leave partial output.
        let before = fs::read(path.join("analysis.json")).unwrap();
        let checks = std::cell::Cell::new(0);
        assert!(
            cache::save_as(&path, &stems, &analysis, format, &|| {
                checks.set(checks.get() + 1);
                checks.get() < 10
            })
            .is_err()
        );
        assert_eq!(fs::read(path.join("analysis.json")).unwrap(), before);
        assert!(
            !fs::read_dir(&p.cache)
                .unwrap()
                .flatten()
                .any(|e| e.file_name().to_string_lossy().ends_with(".partial"))
        );
        let audio = path.join(&manifest.files()[1]);
        let mut damaged = fs::read(&audio).unwrap();
        let index = damaged.len() / 2;
        damaged[index] ^= 0x80;
        fs::write(&audio, damaged).unwrap();
        assert!(p.playback(&hash, frames, RATE).is_err());
        assert!(
            Processor::validate_portable_cache(&path, analysis.duration_sec, &|| true).is_err()
        );
    }
}

#[test]
fn legacy_float_wav_stays_readable_and_unknown_storage_cannot_supply_paths() {
    let root = tempfile::tempdir().unwrap();
    let stems = fixture(4410);
    let analysis = crate::evidence::extract(&stems, vec![]);
    let path = root.path().join("entry");
    cache::save_as(&path, &stems, &analysis, StorageFormat::Wav, &|| true).unwrap();
    fs::write(
        path.join("analysis.json"),
        serde_json::to_vec(&analysis).unwrap(),
    )
    .unwrap();
    Processor::validate_portable_cache(&path, analysis.duration_sec, &|| true).unwrap();
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("analysis.json")).unwrap()).unwrap();
    manifest["storage"] = serde_json::json!({"version": 1, "format": "../../escape"});
    fs::write(
        path.join("analysis.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    assert!(Processor::portable_files(&path).is_err());
    assert!(cache::read(&path, analysis.duration_sec).unwrap().is_none());
}
