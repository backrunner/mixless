//! Playback readiness is independent of beat, key and phrase analysis.
use super::*;

pub(super) struct Playback {
    pub buf: Arc<mixless_engine::AudioBuffer>,
    pub wave: Arc<mixless_protocol::Waveform>,
}

pub(super) fn current_track(core: &AppCore, id: TrackId) -> Result<Track, String> {
    let previous = core.library.get_track(id).map_err(|e| e.to_string())?;
    let mut track = core.library.resolve_track_file(id).map_err(|error| {
        if let mixless_library::LibraryError::MissingFile(path) = &error {
            core.analysis.set(id, Status::MissingFile(path.clone()));
        }
        error.to_string()
    })?;
    if matches!(core.analysis.status(&track), Status::MissingFile(_)) {
        core.analysis.set(id, Status::Checking);
    }
    if track.path != previous.path {
        core.analysis
            .latest
            .lock()
            .expect("analysis metadata")
            .insert(id, track.clone());
        core.analysis.updated(track.clone());
        core.analysis.revision.fetch_add(1, Ordering::Release);
    }
    let path = Path::new(&track.path);
    let hash = core
        .library
        .verified_content_hash(path)
        .map_err(|e| e.to_string())?;
    if track.content_hash != hash {
        core.analysis.forget(id);
        core.library.import_file(path).map_err(|e| e.to_string())?;
        track = core.library.get_track(id).map_err(|e| e.to_string())?;
    }
    Ok(track)
}

pub(super) fn pcm(
    core: &AppCore,
    track: &Track,
) -> Result<Arc<mixless_engine::AudioBuffer>, String> {
    let lock = core
        .analysis
        .audio_locks
        .lock()
        .expect("audio locks")
        .entry(track.id)
        .or_default()
        .clone();
    let _guard = lock.lock().map_err(|_| "Audio preparation lock poisoned")?;
    let cached = core
        .analysis
        .decoded
        .lock()
        .expect("decoded cache")
        .iter()
        .find(|(id, hash, _)| *id == track.id && *hash == track.content_hash)
        .map(|(_, _, buf)| buf.clone());
    let buf = match cached {
        Some(buf) => buf,
        None => mixless_engine::decode_file(Path::new(&track.path)).map_err(|e| e.to_string())?,
    };
    if core
        .library
        .verified_content_hash(Path::new(&track.path))
        .map_err(|e| e.to_string())?
        != track.content_hash
    {
        return Err("File changed while loading; retry".into());
    }
    remember_audio(core, track, buf.clone());
    Ok(buf)
}

pub(super) fn audio(core: &AppCore, track: &Track) -> Result<Playback, String> {
    // Release the decode lock before expensive spectrum filtering and SQL.
    // A manual load can share this PCM while waveform/analysis is in flight.
    let buf = pcm(core, track)?;
    let prepared_wave = core
        .analysis
        .prepared
        .lock()
        .expect("prepared cache")
        .iter()
        .find(|p| p.track.id == track.id && p.track.content_hash == track.content_hash)
        .map(|p| p.wave.clone());
    let wave = if let Some(wave) = prepared_wave {
        wave
    } else {
        match core
            .library
            .load_waveform(track.id)
            .map_err(|e| e.to_string())?
        {
            Some(wave) => Arc::new(wave),
            None => {
                let columns = (buf.frames.div_ceil(32) as usize).clamp(1, 1_048_576);
                let wave = Arc::new(mixless_engine::compute_waveform(&buf, columns));
                core.library
                    .save_waveform(track.id, &track.content_hash, &wave)
                    .map_err(|e| e.to_string())?;
                wave
            }
        }
    };
    remember_audio(core, track, buf.clone());
    Ok(Playback { buf, wave })
}

/// Check fresh engine state and cancel the host's load generation under the
/// publication lock. A rejected request must not alter playback or preparation.
pub fn unload<E>(
    core: &AppCore,
    deck: DeckId,
    check: impl FnOnce(&mixless_protocol::EngineSnapshot) -> Result<(), E>,
) -> Result<(), E> {
    let _commit = core.automix_commit.lock().expect("deck unload commit");
    check(&core.engine.snapshot())?;
    core.engine.eject(deck);
    core.analysis.loaded.lock().expect("loaded analysis")[deck.index()] = None;
    core.analysis
        .playback_revision
        .lock()
        .expect("source revision")[deck.index()] = None;
    Ok(())
}

/// Publish audio with a cheap envelope as soon as decode finishes. Full analysis is attached by
/// a separate worker, guarded by the same load generation and track identity.
pub fn load_manual(
    core: &AppCore,
    deck: DeckId,
    id: TrackId,
    active: impl Fn() -> bool,
) -> Result<Option<String>, String> {
    if !active() {
        return Ok(None);
    }
    let track = current_track(core, id)?;
    let buf = pcm(core, &track)?;
    if !active() {
        return Ok(None);
    }
    let prepared_wave = core
        .analysis
        .prepared
        .lock()
        .expect("prepared cache")
        .iter()
        .find(|p| p.track.id == id && p.track.content_hash == track.content_hash)
        .map(|p| p.wave.clone());
    let wave = prepared_wave
        .unwrap_or_else(|| Arc::new(mixless_engine::compute_preview_waveform(&buf, 4096)));
    let _commit = core
        .automix_commit
        .lock()
        .map_err(|_| "Deck load lock poisoned")?;
    let source = Arc::downgrade(&buf);
    if !core
        .engine
        .load_buffer_if(deck, id, buf, wave, track.title, track.artist, &active)
        .map_err(|e| e.to_string())?
    {
        return Ok(None);
    }
    core.analysis.loaded.lock().expect("loaded analysis")[deck.index()] = None;
    core.analysis
        .playback_revision
        .lock()
        .expect("source revision")[deck.index()] = Some((source, track.content_hash.clone()));
    for cue in core.library.cues(id).map_err(|e| e.to_string())? {
        core.engine.set_cue_frame(deck, cue.index, cue.frame);
        let _ = core.engine.dispatch(mixless_protocol::Command::SetCueKind {
            track_id: id,
            index: cue.index,
            kind: if cue.user_set {
                cue.kind
            } else {
                mixless_protocol::CueKind::Hot
            },
        });
    }
    Ok(Some(track.content_hash))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unload_cancels_a_preparing_load_and_preserves_library_and_other_deck() {
        use std::sync::{atomic::AtomicU64, mpsc::channel};
        use std::time::Duration;

        let (_dir, core, tracks) = crate::automix::tests::fixture();
        load(&core, DeckId::A, tracks[0], || true).unwrap();
        load(&core, DeckId::B, tracks[1], || true).unwrap();
        core.engine
            .dispatch(mixless_protocol::Command::PlayPause { deck: DeckId::B })
            .unwrap();
        let cues = core.library.cues(tracks[0]).unwrap();
        let other = core.engine.snapshot().decks[1].clone();

        let epoch = Arc::new(AtomicU64::new(0));
        let worker_epoch = epoch.clone();
        let worker_core = core.clone();
        let next = tracks[2];
        let (ready_tx, ready_rx) = channel();
        let (resume_tx, resume_rx) = channel();
        let worker = std::thread::spawn(move || {
            let checks = AtomicU64::new(0);
            load_manual(&worker_core, DeckId::A, next, || {
                if checks.fetch_add(1, Ordering::Relaxed) == 1 {
                    // Decode completed; pause just before the publication step.
                    ready_tx.send(()).unwrap();
                    resume_rx.recv_timeout(Duration::from_secs(10)).unwrap();
                }
                worker_epoch.load(Ordering::Acquire) == 0
            })
        });
        ready_rx.recv_timeout(Duration::from_secs(10)).unwrap();
        epoch.fetch_add(1, Ordering::AcqRel);
        unload(&core, DeckId::A, |_| Ok::<_, ()>(())).unwrap();
        resume_tx.send(()).unwrap();
        assert!(worker.join().unwrap().unwrap().is_none());

        assert!(core.engine.snapshot().decks[0].track_id.is_none());
        assert!(core.analysis.loaded.lock().unwrap()[0].is_none());
        assert!(core.analysis.playback_revision.lock().unwrap()[0].is_none());
        assert_eq!(core.engine.snapshot().decks[1], other);
        assert!(core.analysis.loaded.lock().unwrap()[1].is_some());
        assert!(core.library.get_track(tracks[0]).is_ok());
        assert_eq!(
            serde_json::to_value(core.library.cues(tracks[0]).unwrap()).unwrap(),
            serde_json::to_value(cues).unwrap()
        );
    }
}
