//! Called by load/model workers. No inference or file reads on the UI/audio thread.
use super::*;

pub fn attach_stems(core: &Arc<AppCore>, deck: DeckId, id: TrackId, hash: &str) {
    let Some(processor) = &core.stems else {
        return;
    };
    let Some(source) = core.engine.stem_source(deck, id) else {
        return;
    };
    let matches = core
        .analysis
        .playback_revision
        .lock()
        .expect("source revision")[deck.index()]
    .as_ref()
    .is_some_and(|(buffer, revision)| {
        revision == hash
            && buffer
                .upgrade()
                .is_some_and(|buffer| Arc::ptr_eq(&buffer, &source))
    });
    if !matches {
        return;
    }
    if core.engine.stems_ready(deck) {
        return;
    }
    match processor.playback(hash, source.frames, source.sample_rate) {
        Ok(Some(stems)) => {
            if let Err(error) = core.engine.attach_stems(deck, id, &source, stems) {
                tracing::warn!(%error,"Cannot attach stem playback");
            }
        }
        Ok(None) => deep::retry_for_playback(core, id),
        Err(error) => tracing::warn!(%error,"Stem playback unavailable; retaining original audio"),
    }
}

#[cfg(test)]
pub(super) fn verify_native_stem_playback_under_inference(core: &Arc<AppCore>, id: TrackId) {
    use mixless_protocol::{Command, StemKind};
    load(core, DeckId::B, id, || true).unwrap();
    assert!(core.engine.stems_ready(DeckId::A) && core.engine.stems_ready(DeckId::B));
    core.engine
        .dispatch(Command::SetCrossfader { value: 0. })
        .unwrap();
    for deck in [DeckId::A, DeckId::B] {
        core.engine
            .dispatch(Command::SetStemGain {
                deck,
                stem: StemKind::Vocals,
                value: 0.3,
            })
            .unwrap();
        core.engine
            .dispatch(Command::SetStemGain {
                deck,
                stem: StemKind::Drums,
                value: 0.65,
            })
            .unwrap();
        core.engine
            .dispatch(Command::SetRate { deck, rate: 1.06 })
            .unwrap();
        core.engine
            .dispatch(Command::SetPitchSemitones {
                deck,
                semitones: if deck == DeckId::A { 1. } else { -1. },
            })
            .unwrap();
        core.engine
            .dispatch(Command::SetChannelFader { deck, value: 0.8 })
            .unwrap();
        core.engine
            .dispatch(Command::SetCue {
                deck,
                index: 0,
                frame: 44100,
            })
            .unwrap();
        core.engine
            .dispatch(Command::JumpCue { deck, index: 0 })
            .unwrap();
        core.engine
            .dispatch(Command::SetLoopBeats {
                deck,
                beats: 4.,
                on: true,
            })
            .unwrap();
        if !core.engine.snapshot().deck(deck).playing {
            core.engine.dispatch(Command::PlayPause { deck }).unwrap();
        }
    }
    let source = core.engine.stem_source(DeckId::A, id).unwrap();
    let worker_core = core.clone();
    let worker = std::thread::spawn(move || {
        worker_core.stems.as_ref().unwrap().analyze(
            "concurrent-native-probe",
            source.frames as f32 / source.sample_rate as f32,
            || Ok(source),
            &mut |_| {},
            &|| true,
        )
    });
    let mut timings = Vec::new();
    let began = std::time::Instant::now();
    let mut peak = 0f32;
    while !worker.is_finished() {
        let at = std::time::Instant::now();
        let audio = core.engine.render_offline(128);
        timings.push(at.elapsed().as_secs_f64() * 1000.);
        for sample in audio {
            assert!(sample.is_finite() && sample.abs() <= 1.);
            peak = peak.max(sample.abs());
        }
        assert!(began.elapsed().as_secs() < 600);
        std::thread::sleep(std::time::Duration::from_micros(2667));
    }
    assert!(worker.join().unwrap().is_ok());
    assert!(peak > 0.001);
    timings.sort_by(f64::total_cmp);
    let p99 = timings[timings.len() * 99 / 100];
    eprintln!(
        "Two native stem decks + keylock/pitch during fresh inference: {} blocks, p99 {p99:.3} ms / 2.667 ms",
        timings.len()
    );
    assert!(p99 < 2.667 * 0.5);
}
