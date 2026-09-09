//! Full file -> analysis -> transport regression. Expected phase comes from
//! the generated audio's drum times, independently of the detected beat grids.
use mixless_analyze::Analyzer;
use mixless_engine::{Engine, EngineConfig};
use mixless_protocol::{Command, DeckId, TrackId};

struct Fixture(std::path::PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn analyzed_files_automatically_match_actual_drum_beats() {
    let dir =
        Fixture(std::env::temp_dir().join(format!("mixless-beat-sync-{}", std::process::id())));
    std::fs::create_dir_all(&dir.0).unwrap();
    let rates = [44_100, 48_000];
    let bpms = [123.0_f64, 128.5];
    let offsets = [0.13_f64, 0.37];
    let engine = Engine::new(EngineConfig {
        offline: true,
        ..Default::default()
    })
    .unwrap();
    for (i, deck) in [DeckId::A, DeckId::B].into_iter().enumerate() {
        let path = dir.0.join(format!("{i}.wav"));
        let mut writer = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: rates[i],
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .unwrap();
        for n in 0..rates[i] * 40 {
            let time = n as f64 / rates[i] as f64 - offsets[i];
            let beat = time * bpms[i] / 60.0;
            let phase = beat.rem_euclid(1.0) * 60.0 / bpms[i];
            let accent = if beat.floor() as i64 % 4 == 0 {
                0.7
            } else {
                0.4
            };
            let value = if time < 0.0 {
                0.0
            } else {
                accent * (-phase * 40.0).exp() * (std::f64::consts::TAU * 70.0 * phase).sin()
            } as f32;
            writer.write_sample(value).unwrap();
            writer.write_sample(value).unwrap();
        }
        writer.finalize().unwrap();
        let id = TrackId(i as i64 + 1);
        let analysis = Analyzer::new().analyze_track(id, &path).unwrap();
        assert!((analysis.tempo.global_bpm as f64 - bpms[i]).abs() < 0.2);
        engine
            .load_file(deck, id, &path, format!("Drums {i}"), String::new())
            .unwrap();
        engine.set_beat_grid(deck, id, analysis.tempo).unwrap();
        engine
            .dispatch(Command::Jog {
                deck,
                delta_frames: (rates[i] as f64 * (2.0 + i as f64 * 0.19)) as f32,
            })
            .unwrap();
        engine.dispatch(Command::PlayPause { deck }).unwrap();
    }
    engine.render_offline(256);
    engine
        .dispatch(Command::Sync {
            deck: DeckId::B,
            keylock: true,
        })
        .unwrap();
    for block in 0..1000 {
        let output = engine.render_offline(1024);
        assert!(output.iter().all(|x| x.is_finite()));
        if block % 100 == 99 {
            let snapshot = engine.snapshot();
            let beats: [f64; 2] = std::array::from_fn(|i| {
                (snapshot.decks[i].frame as f64 / rates[i] as f64 - offsets[i]) * bpms[i] / 60.0
            });
            let error = (beats[0] - beats[1] + 0.5).rem_euclid(1.0) - 0.5;
            assert!(
                error.abs() < 0.025,
                "block {block}: actual drum phase {error} beats"
            );
            assert!(snapshot.decks[1].sync_locked);
        }
    }
}
