use super::*;
use mixless_protocol::{Command, CueKind};

impl AnalysisJobs {
    pub fn queue_reanalysis(&self, id: TrackId) {
        self.epoch(id).fetch_add(1, Ordering::AcqRel);
        self.set(id, Status::Queued);
    }
}

pub fn reanalyze(core: &AppCore, id: TrackId, clear_cues: bool) -> Result<(), String> {
    let lock = core
        .analysis
        .locks
        .lock()
        .expect("analysis locks")
        .entry(id)
        .or_default()
        .clone();
    let _track = lock.lock().map_err(|_| "Analysis lock poisoned")?;
    if let Some(stems) = &core.stems {
        stems.retry();
    }
    // Serialize with an existing analysis so old cues cannot arrive after reset.
    core.library
        .reset_analysis(id, clear_cues)
        .map_err(|e| e.to_string())?;
    core.analysis.forget(id);
    core.analysis
        .content_revision
        .fetch_add(1, Ordering::AcqRel);
    core.analysis.set(id, Status::Analyzing);
    let result = prepare_inner(core, id);
    let prepared = match result {
        Ok(p) => p,
        Err(error) => {
            core.analysis.set(id, Status::Failed(error.clone()));
            return Err(error);
        }
    };
    core.analysis
        .latest
        .lock()
        .expect("metadata")
        .insert(id, prepared.track.clone());
    core.analysis
        .updates
        .lock()
        .expect("metadata updates")
        .insert(id, prepared.track.clone());
    core.analysis.set(id, super::deep::ready_status(&prepared));
    update_loaded(core, &prepared, true)
}

// Refresh cue metadata without seeking or changing a running transition. Stem
// analysis retains the original beat grid; explicit reanalysis may replace it.
pub(super) fn update_loaded(
    core: &AppCore,
    prepared: &PreparedTrack,
    refresh_grid: bool,
) -> Result<(), String> {
    let id = prepared.track.id;
    let cues = core.library.cues(id).map_err(|e| e.to_string())?;
    let _commit = core
        .automix_commit
        .lock()
        .map_err(|_| "Deck load lock poisoned")?;
    for deck in [DeckId::A, DeckId::B] {
        if core.engine.snapshot().deck(deck).track_id != Some(id) {
            continue;
        }
        for index in 0..8 {
            core.engine
                .dispatch(Command::ClearCue { deck, index })
                .map_err(|e| e.to_string())?;
        }
        for cue in &cues {
            core.engine.set_cue_frame(deck, cue.index, cue.frame);
            core.engine
                .dispatch(Command::SetCueKind {
                    track_id: id,
                    index: cue.index,
                    kind: if cue.user_set { cue.kind } else { CueKind::Hot },
                })
                .map_err(|e| e.to_string())?;
        }
        if refresh_grid && prepared.analysis.tempo.beats.len() >= 2 {
            core.engine
                .set_beat_grid(deck, id, prepared.analysis.tempo.clone())
                .map_err(|e| e.to_string())?;
        }
        core.analysis.loaded.lock().expect("loaded analysis")[deck.index()] =
            Some(prepared.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reanalysis_refreshes_loaded_cues_without_interrupting_audio() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        core.library
            .set_cue(id, 7, 48000, CueKind::In, true)
            .unwrap();
        load(&core, DeckId::A, id, || true).unwrap();
        core.engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        core.engine.render_offline(4800);
        let before = core.engine.snapshot().decks[0].frame;
        reanalyze(&core, id, false).unwrap();
        assert!(
            core.library
                .cues(id)
                .unwrap()
                .iter()
                .any(|c| c.index == 7 && c.user_set)
        );
        assert!(core.engine.snapshot().decks[0].cues[7].is_some());
        reanalyze(&core, id, true).unwrap();
        assert!(core.library.cues(id).unwrap().iter().all(|c| !c.user_set));
        assert!(core.engine.snapshot().decks[0].cues[7].is_none());
        assert!(core.engine.snapshot().decks[0].playing);
        assert_eq!(core.engine.snapshot().decks[0].frame, before);
        assert!(
            core.engine
                .render_offline(480)
                .iter()
                .any(|v| v.abs() > 0.01)
        );
    }
}
