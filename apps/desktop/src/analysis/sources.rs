use super::*;

pub fn relink_file(core: &Arc<AppCore>, id: TrackId, path: &Path) -> Result<(), String> {
    let lock = core
        .analysis
        .locks
        .lock()
        .expect("analysis locks")
        .entry(id)
        .or_default()
        .clone();
    let guard = lock.lock().map_err(|_| "Analysis lock poisoned")?;
    let track = core.library.get_track(id).map_err(|e| e.to_string())?;
    let previous = core.analysis.status(&track);
    core.analysis.set(id, Status::Checking);
    // Validate before changing the saved source. A bad selection leaves the
    // missing-file row and its recovery actions intact.
    let validated = (|| {
        let hash = core
            .library
            .verified_content_hash(path)
            .map_err(|e| e.to_string())?;
        let audio = mixless_engine::decode_file(path)
            .map_err(|e| format!("Cannot use this audio file: {e}"))?;
        let track = core
            .library
            .relink_track_file(&track, path, &hash)
            .map_err(|e| e.to_string())?;
        Ok::<_, String>((track, audio))
    })();
    let (track, audio) = match validated {
        Ok(result) => result,
        Err(error) => {
            core.analysis.set(id, previous);
            return Err(error);
        }
    };
    core.analysis.forget(id);
    core.analysis
        .content_revision
        .fetch_add(1, Ordering::AcqRel);
    core.analysis.updated(track.clone());
    remember_audio(core, &track, audio);
    core.analysis.set(id, Status::Queued);
    drop(guard);
    prepare(core, id)?;
    deep::schedule(core, id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moved_audio_reuses_analysis_and_updates_the_cached_path_for_playback() {
        let (dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let old = core.library.get_track(id).unwrap();
        let moved = dir.path().join("moved/renamed.wav");
        std::fs::create_dir_all(moved.parent().unwrap()).unwrap();
        std::fs::rename(&old.path, &moved).unwrap();
        let prepared = prepare(&core, id).unwrap();
        assert_eq!(
            Path::new(&prepared.track.path),
            moved.canonicalize().unwrap()
        );
        assert_eq!(prepared.track.content_hash, old.content_hash);
        assert_eq!(core.analysis.take_updates()[&id].path, prepared.track.path);
        load(&core, DeckId::A, id, || true).unwrap();
        assert_eq!(core.engine.snapshot().decks[0].track_id, Some(id));
    }

    #[test]
    fn missing_audio_has_recovery_status_and_retry_preserves_cached_analysis() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let old = core.library.get_track(id).unwrap();
        let bytes = std::fs::read(&old.path).unwrap();
        std::fs::remove_file(&old.path).unwrap();
        assert!(prepare(&core, id).err().unwrap().contains("File not found"));
        assert_eq!(
            core.analysis.status(&old),
            Status::MissingFile(old.path.clone())
        );
        assert!(core.analysis.summary().contains("1 file not found"));
        assert!(!core.analysis.summary().contains("remaining"));
        assert!(reanalyze(&core, id, false).is_err());
        assert!(core.library.get_track(id).unwrap().analyzed);
        std::fs::write(&old.path, bytes).unwrap();
        assert!(prepare(&core, id).is_ok());
        assert!(matches!(core.analysis.status(&old), Status::Ready(_)));
    }

    #[test]
    fn manual_selection_is_validated_before_relink_and_resumes_analysis() {
        let (dir, core, tracks) = crate::automix::tests::fixture();
        let elsewhere = tempfile::tempdir().unwrap();
        let id = tracks[0];
        let old = core.library.get_track(id).unwrap();
        let moved = elsewhere.path().join("found.wav");
        std::fs::rename(&old.path, &moved).unwrap();
        assert!(prepare(&core, id).is_err());
        let invalid = dir.path().join("invalid.wav");
        std::fs::write(&invalid, b"not audio").unwrap();
        assert!(relink_file(&core, id, &invalid).is_err());
        assert_eq!(core.library.get_track(id).unwrap().path, old.path);
        assert!(matches!(core.analysis.status(&old), Status::MissingFile(_)));
        core.library
            .set_cue(id, 7, 200, mixless_protocol::CueKind::In, true)
            .unwrap();
        relink_file(&core, id, &moved).unwrap();
        let linked = core.library.get_track(id).unwrap();
        assert_eq!(Path::new(&linked.path), moved.canonicalize().unwrap());
        assert!(matches!(core.analysis.status(&linked), Status::Ready(_)));
        assert!(
            core.library
                .cues(id)
                .unwrap()
                .iter()
                .any(|cue| cue.user_set && cue.index == 7)
        );
        load_manual(&core, DeckId::A, id, || true).unwrap();
        assert_eq!(core.engine.snapshot().decks[0].track_id, Some(id));
    }

    #[test]
    fn removal_evicts_status_and_late_queued_analysis_cannot_restore_it() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        remove_track(&core, id).unwrap();
        assert!(prepare(&core, id).is_err());
        assert!(!core.analysis.statuses().contains_key(&id));
        assert!(!core.analysis.latest.lock().unwrap().contains_key(&id));
    }
}

pub fn remove_track(core: &AppCore, id: TrackId) -> Result<(), String> {
    let lock = core
        .analysis
        .locks
        .lock()
        .expect("analysis locks")
        .entry(id)
        .or_default()
        .clone();
    let _guard = lock.lock().map_err(|_| "Analysis lock poisoned")?;
    core.library.remove_track(id).map_err(|e| e.to_string())?;
    core.analysis.forget(id);
    core.analysis
        .decoded
        .lock()
        .expect("decoded cache")
        .retain(|(track, _, _)| *track != id);
    core.analysis
        .content_revision
        .fetch_add(1, Ordering::AcqRel);
    Ok(())
}
