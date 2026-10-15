use super::*;
use mixless_engine::{AudioBuffer, Engine, EngineConfig, StemBuffer};
use mixless_protocol::{Command, DeckId, FxParams, FxSlot, StemKind, TrackId};

fn engine(pcm: Vec<f32>, rate: u32, stems: Option<Arc<StemBuffer>>) -> Engine {
    let engine = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48000,
        block_frames: 128,
    })
    .unwrap();
    let source = Arc::new(AudioBuffer {
        frames: pcm.len() as u64 / 2,
        samples: pcm,
        sample_rate: rate,
        loudness: Default::default(),
    });
    let wave = Arc::new(mixless_engine::compute_preview_waveform(&source, 32));
    engine
        .load_buffer_if(
            DeckId::A,
            TrackId(1),
            source.clone(),
            wave,
            "codec fixture".into(),
            "".into(),
            || true,
        )
        .unwrap();
    if let Some(stems) = stems {
        assert!(
            engine
                .attach_stems(DeckId::A, TrackId(1), &source, stems)
                .unwrap()
        );
    }
    engine
        .dispatch(Command::SetCrossfader { value: -1. })
        .unwrap();
    engine
}

fn render(e: &Engine, frames: usize) -> Vec<f32> {
    let mut pcm = Vec::new();
    for start in (0..frames).step_by(128) {
        pcm.extend(e.render_offline((frames - start).min(128)));
    }
    pcm
}

fn compare(a: &Engine, b: &Engine) {
    let left = render(a, 6000);
    let right = render(b, 6000);
    assert!(right.iter().all(|s| s.is_finite() && s.abs() <= 1.));
    let error = left
        .iter()
        .zip(&right)
        .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
        .sum::<f64>()
        / left.len() as f64;
    assert!(
        error.sqrt() < 1e-6,
        "engine-path RMS error {}",
        error.sqrt()
    );
    assert_eq!(a.snapshot().decks[0].frame, b.snapshot().decks[0].frame);
}

#[test]
fn decoded_formats_share_transport_stretch_fx_and_unity_original() {
    let dir = tempfile::tempdir().unwrap();
    let p = Processor::new(dir.path().join("models"), dir.path().join("cache"), false);
    let stems = fixture(44100 * 4 + 317);
    let analysis = crate::evidence::extract(&stems, vec![]);
    let mix: Vec<f32> = (0..stems.bass.len())
        .map(|i| stems.audio.iter().map(|s| s[i]).sum::<f32>() + stems.bass[i])
        .collect();
    for format in [StorageFormat::Wav, StorageFormat::Flac, StorageFormat::Mp3] {
        let path = p.cache_path("render");
        cache::save_as(&path, &stems, &analysis, format, &|| true).unwrap();
        let manifest = cache::manifest(&path).unwrap();
        let voice = cache::audio(&path, &manifest, 0, &|| true).unwrap();
        for rate in [44100, 48000] {
            let original = crate::resample::convert(&mix, 2, RATE, rate);
            let voice = crate::resample::convert(&voice, 2, RATE, rate);
            for isolate in [false, true] {
                let loaded = p
                    .playback("render", original.len() as u64 / 2, rate)
                    .unwrap()
                    .unwrap();
                let reference = engine(
                    if isolate {
                        voice.clone()
                    } else {
                        original.clone()
                    },
                    rate,
                    None,
                );
                let actual = engine(original.clone(), rate, Some(loaded));
                if isolate {
                    for stem in [StemKind::Drums, StemKind::Instruments, StemKind::Bass] {
                        actual
                            .dispatch(Command::SetStemGain {
                                deck: DeckId::A,
                                stem,
                                value: 0.,
                            })
                            .unwrap();
                    }
                }
                for e in [&reference, &actual] {
                    // Settle manual gain ramps while paused, before either
                    // stretcher consumes audio with different initial gains.
                    render(e, 12000);
                    for cmd in [
                        Command::SetRate {
                            deck: DeckId::A,
                            rate: 1.08,
                        },
                        Command::SetPitchSemitones {
                            deck: DeckId::A,
                            semitones: -2.,
                        },
                        Command::SetCue {
                            deck: DeckId::A,
                            index: 0,
                            frame: rate as u64 / 2,
                        },
                        Command::SetFx {
                            deck: Some(DeckId::A),
                            slot: FxSlot::INSERTS[0],
                            params: FxParams {
                                kind: Some("reverb".into()),
                                mix: 0.2,
                                ..Default::default()
                            },
                        },
                        Command::PlayPause { deck: DeckId::A },
                    ] {
                        e.dispatch(cmd).unwrap();
                    }
                    render(e, 12000);
                }
                compare(&reference, &actual);
                for cmd in [
                    Command::JumpCue {
                        deck: DeckId::A,
                        index: 0,
                    },
                    Command::SetLoopBeats {
                        deck: DeckId::A,
                        beats: 2.,
                        on: true,
                    },
                    Command::SetReverse {
                        deck: DeckId::A,
                        on: true,
                    },
                ] {
                    for e in [&reference, &actual] {
                        e.dispatch(cmd.clone()).unwrap();
                        render(e, 12000);
                    }
                    compare(&reference, &actual);
                }
            }
        }
    }
}
