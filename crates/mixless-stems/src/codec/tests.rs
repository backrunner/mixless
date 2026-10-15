use super::*;
use crate::{Error, StorageFormat, storage::RATE};
use std::path::Path;

#[test]
#[ignore = "Regenerate synthetic mixpack codec fixtures"]
fn generate_portable_fixtures() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mixless-pack/src/fixtures");
    std::fs::create_dir_all(&dir).unwrap();
    for format in [StorageFormat::Flac, StorageFormat::Mp3] {
        write(
            &dir.join(format!("stem.{}", format.extension())),
            &vec![0.05; 8820],
            format,
            &|| true,
        )
        .unwrap();
    }
}

#[test]
fn formats_preserve_frames_channels_headroom_and_transient_positions() {
    let root = tempfile::tempdir().unwrap();
    // Non-MP3-block-aligned length, distinct channels and impulses near both ends.
    let frames = 44100 * 2 + 317;
    let mut pcm = vec![0.; frames * 2];
    let positions = [101, 43109, frames - 151];
    for &i in &positions {
        pcm[i * 2] = 1.75;
        pcm[(i + 31) * 2 + 1] = -1.25;
    }
    for format in [StorageFormat::Wav, StorageFormat::Flac, StorageFormat::Mp3] {
        let path = root.path().join(format!("test.{}", format.extension()));
        let gain = write(&path, &pcm, format, &|| true).unwrap();
        if let Some(dir) = std::env::var_os("MIXLESS_CODEC_FIXTURE") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::copy(&path, Path::new(&dir).join(path.file_name().unwrap())).unwrap();
        }
        let decoded = read(&path, format, frames as u64, gain, &|| true)
            .unwrap_or_else(|e| panic!("{format:?}: {e}"));
        assert_eq!(decoded.len(), pcm.len());
        for &i in &positions {
            for (channel, expected) in [(0, i), (1, i + 31)] {
                let peak = (expected - 50..=expected + 50)
                    .max_by(|&a, &b| {
                        decoded[a * 2 + channel]
                            .abs()
                            .total_cmp(&decoded[b * 2 + channel].abs())
                    })
                    .unwrap();
                assert_eq!(peak, expected, "{format:?} channel {channel}");
            }
        }
        if format != StorageFormat::Mp3 {
            assert_eq!(
                decoded, pcm,
                "exact representable values with restored headroom"
            );
        }
    }
}

#[test]
fn flac_rounding_is_bounded_and_mp3_retains_signal_at_320_kbps() {
    let root = tempfile::tempdir().unwrap();
    let frames = 44100 * 3 + 991;
    let pcm: Vec<_> = (0..frames)
        .flat_map(|i| {
            let t = i as f32 / RATE as f32;
            [
                0.5 * (t * 331. * std::f32::consts::TAU).sin(),
                0.3 * (t * 713. * std::f32::consts::TAU).sin(),
            ]
        })
        .collect();
    for format in [StorageFormat::Flac, StorageFormat::Mp3] {
        let path = root.path().join(format!("test.{}", format.extension()));
        let gain = write(&path, &pcm, format, &|| true).unwrap();
        let decoded = read(&path, format, frames as u64, gain, &|| true)
            .unwrap_or_else(|e| panic!("{format:?}: {e}"));
        let peak = pcm
            .iter()
            .zip(&decoded)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        let signal = pcm.iter().map(|s| (*s as f64).powi(2)).sum::<f64>();
        let noise = pcm
            .iter()
            .zip(&decoded)
            .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
            .sum::<f64>();
        let snr = 10. * (signal / noise).log10();
        eprintln!("{format:?}: peak error {peak:.9}, SNR {snr:.2} dB");
        if format == StorageFormat::Flac {
            assert!(peak <= gain / 16777216.);
        } else {
            assert!(snr > 40., "synthetic stereo tone fidelity");
            let bytes = std::fs::metadata(path).unwrap().len() as f64;
            let kbps = bytes * 8. / (frames as f64 / RATE as f64) / 1000.;
            assert!(
                (320.0..340.0).contains(&kbps),
                "{kbps} kbps including header/padding"
            );
        }
    }
}

#[test]
fn silence_short_final_blocks_and_cancellation_are_supported() {
    let root = tempfile::tempdir().unwrap();
    for format in [StorageFormat::Wav, StorageFormat::Flac, StorageFormat::Mp3] {
        for frames in [1, 47, 1152, 1153, 4096, 8192] {
            let path = root.path().join(format!("silence.{}", format.extension()));
            let gain = write(&path, &vec![0.; frames * 2], format, &|| true).unwrap();
            let decoded = read(&path, format, frames as u64, gain, &|| true)
                .unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert_eq!(decoded, vec![0.; frames * 2]);
            assert!(matches!(
                read(&path, format, frames as u64, gain, &|| false),
                Err(Error::Cancelled)
            ));
        }
    }
}

#[test]
fn flac_block_boundaries_retain_full_scale_stereo_pcm() {
    let dir = tempfile::tempdir().unwrap();
    for tail in [0, 1, 15, 16, 4095] {
        let frames = 4096 * 32 + tail;
        let mut seed = 17u32;
        let pcm: Vec<f32> = (0..frames * 2)
            .map(|i| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                if i / 8192 % 3 == 0 {
                    0.
                } else {
                    (seed as i32 as f64 / i32::MAX as f64 * 1.5) as f32
                }
            })
            .collect();
        let path = dir.path().join("blocks.flac");
        let gain = write(&path, &pcm, StorageFormat::Flac, &|| true).unwrap();
        let decoded = read(&path, StorageFormat::Flac, frames as u64, gain, &|| true).unwrap();
        assert!(
            pcm.iter()
                .zip(decoded)
                .all(|(a, b)| (a - b).abs() <= gain / 16777216.)
        );
    }
}
