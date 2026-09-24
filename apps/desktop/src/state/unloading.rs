//! Unload only the track the user opened the menu for. Both mouse and
//! keyboard actions recheck under the same lock as manual/AutoMix publication.
use super::*;

#[derive(Clone, Copy, Debug)]
pub struct DeckUnloadTarget {
    pub deck: DeckId,
    track: Option<TrackId>,
    load_generation: u64,
    source_revision: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnloadBlock {
    Empty,
    Changed,
}

impl UnloadBlock {
    pub fn message(self) -> &'static str {
        match self {
            Self::Empty => "No track loaded.",
            Self::Changed => "Track changed. Reopen this menu.",
        }
    }
}

impl DeckUnloadTarget {
    fn check(
        self,
        snapshot: &EngineSnapshot,
        generation: u64,
        source_revision: u64,
        loading: bool,
        automix_active: bool,
    ) -> Result<(), UnloadBlock> {
        let current = snapshot.deck(self.deck);
        if self.load_generation != generation
            || self.source_revision != source_revision
            || self.track != current.track_id
        {
            return Err(UnloadBlock::Changed);
        }
        // AUTO owns both decks while bootstrapping and preparing its next
        // source, even when this particular deck is still empty.
        if current.track_id.is_none() && !loading && !automix_active && !snapshot.automix_on {
            return Err(UnloadBlock::Empty);
        }
        Ok(())
    }
}

fn apply_unload(
    core: &AppCore,
    target: DeckUnloadTarget,
    generation: &AtomicU64,
    automix_generation: &AtomicU64,
    loading: bool,
    automix_active: bool,
) -> Result<(), UnloadBlock> {
    crate::analysis::unload(core, target.deck, |snapshot| {
        target.check(
            snapshot,
            generation.load(Ordering::Acquire),
            core.engine.deck_load_revision(target.deck),
            loading,
            automix_active,
        )?;
        // Once the check succeeds, a worker already publishing has finished,
        // and workers waiting to publish will observe cancellation.
        generation.fetch_add(1, Ordering::AcqRel);
        automix_generation.fetch_add(1, Ordering::AcqRel);
        Ok(())
    })
}

impl UiState {
    pub fn deck_unload_target(&self, deck: DeckId) -> DeckUnloadTarget {
        let _commit = self.core.automix_commit.lock().expect("deck menu target");
        DeckUnloadTarget {
            deck,
            track: self.core.engine.snapshot().deck(deck).track_id,
            load_generation: self.deck_load_epoch[deck.index()].load(Ordering::Acquire),
            source_revision: self.core.engine.deck_load_revision(deck),
        }
    }

    pub fn unload_block(&self, target: DeckUnloadTarget) -> Option<UnloadBlock> {
        target
            .check(
                &self.core.engine.snapshot(),
                self.deck_load_epoch[target.deck.index()].load(Ordering::Acquire),
                self.core.engine.deck_load_revision(target.deck),
                self.deck_loading[target.deck.index()].is_some(),
                self.automix_active,
            )
            .err()
    }

    pub fn unload_deck(&mut self, target: DeckUnloadTarget, cx: &mut Context<Self>) {
        let deck = target.deck;
        let index = deck.index();
        if let Err(reason) = apply_unload(
            &self.core,
            target,
            &self.deck_load_epoch[index],
            &self.automix_epoch,
            self.deck_loading[index].is_some(),
            self.automix_active,
        ) {
            self.error = reason.message().into();
            cx.notify();
            return;
        }
        self.automix_active = false;
        self.automix_retry = None;
        self.automix_status.clear();
        self.automix_plan = None;
        self.automix_rx = None;
        self.cancel_deck_transport_press(deck);
        if matches!(self.drag, Some(DragCtl::Jog { deck: d, .. } | DragCtl::Wave { deck: d, .. }) if d == deck)
        {
            self.end_drag();
        }
        if self.momentary_fx.is_some_and(|(d, _, _)| d == deck) {
            self.end_momentary_fx();
        }
        self.sync_request = None;
        self.pending_play[index] = false;
        self.deck_load_rx[index] = None;
        self.deck_loading[index] = None;
        self.grid_rx[index] = None;
        self.wave[index] = None;
        self.wave_cache[index] = None;
        self.wave_cache_rx[index] = None;
        self.wave_tempo[index] = None;
        self.deck_artwork[index] = Default::default();
        self.presentation_frames[index] = 0.;
        self.cue_shift[index] = false;
        self.cue_role_editor[index] = None;
        self.deck_menu = None;
        self.snapshot = self.core.engine.snapshot();
        // Notify before root render so an idle cached table drops its badge.
        self.library_key = None;
        self.library_view.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(core: &AppCore, deck: DeckId, generation: u64) -> DeckUnloadTarget {
        DeckUnloadTarget {
            deck,
            track: core.engine.snapshot().deck(deck).track_id,
            load_generation: generation,
            source_revision: core.engine.deck_load_revision(deck),
        }
    }

    #[test]
    fn playing_unload_cancels_auto_and_only_the_selected_deck_load_generation() {
        for deck in [DeckId::A, DeckId::B] {
            let (_dir, core, tracks) = crate::automix::tests::fixture();
            for loaded in [DeckId::A, DeckId::B] {
                crate::analysis::load(&core, loaded, tracks[loaded.index()], || true).unwrap();
                core.engine
                    .dispatch(Command::PlayPause { deck: loaded })
                    .unwrap();
            }
            core.engine.render_offline(256);
            let generations = [AtomicU64::new(7), AtomicU64::new(7)];
            let auto = AtomicU64::new(12);
            let request = target(&core, deck, 7);
            apply_unload(
                &core,
                request,
                &generations[deck.index()],
                &auto,
                true,
                true,
            )
            .unwrap();
            let other = 1 - deck.index();
            assert_eq!(generations[deck.index()].load(Ordering::Acquire), 8);
            assert_eq!(generations[other].load(Ordering::Acquire), 7);
            assert_eq!(auto.load(Ordering::Acquire), 13);
            let snapshot = core.engine.snapshot();
            assert!(snapshot.deck(deck).track_id.is_none());
            assert!(snapshot.decks[other].playing);
            assert_eq!(snapshot.decks[other].track_id, Some(tracks[other]));
            assert_eq!(
                apply_unload(
                    &core,
                    request,
                    &generations[deck.index()],
                    &auto,
                    false,
                    false
                ),
                Err(UnloadBlock::Changed)
            );
            assert_eq!(
                auto.load(Ordering::Acquire),
                13,
                "duplicate click must not cancel a new session"
            );
        }
    }

    #[test]
    fn menu_cannot_unload_a_new_track_a_same_track_reload_or_a_new_load_request() {
        for replacement in [0, 1, 2] {
            let (_dir, core, tracks) = crate::automix::tests::fixture();
            crate::analysis::load_manual(&core, DeckId::A, tracks[0], || true).unwrap();
            let generation = AtomicU64::new(3);
            let auto = AtomicU64::new(4);
            let request = target(&core, DeckId::A, 3);
            if replacement == 2 {
                generation.fetch_add(1, Ordering::AcqRel);
            } else {
                crate::analysis::load_manual(&core, DeckId::A, tracks[replacement], || true)
                    .unwrap();
            }
            core.engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
            let before = core.engine.snapshot();
            assert_eq!(
                apply_unload(&core, request, &generation, &auto, true, true),
                Err(UnloadBlock::Changed)
            );
            assert_eq!(core.engine.snapshot(), before);
            assert_eq!(auto.load(Ordering::Acquire), 4);
        }
    }

    #[test]
    fn an_empty_deck_can_cancel_manual_loading_or_auto_bootstrap_but_idle_is_noop() {
        let (_dir, core, _) = crate::automix::tests::fixture();
        let generation = AtomicU64::new(0);
        let auto = AtomicU64::new(0);
        let request = target(&core, DeckId::A, 0);
        assert_eq!(
            apply_unload(&core, request, &generation, &auto, false, false),
            Err(UnloadBlock::Empty)
        );
        assert_eq!(generation.load(Ordering::Acquire), 0);
        for (loading, active) in [(true, false), (false, true)] {
            let request = target(&core, DeckId::A, generation.load(Ordering::Acquire));
            apply_unload(&core, request, &generation, &auto, loading, active).unwrap();
            assert!(core.engine.snapshot().decks[0].track_id.is_none());
        }
        assert_eq!(generation.load(Ordering::Acquire), 2);
        assert_eq!(auto.load(Ordering::Acquire), 2);
    }

    #[test]
    fn late_reanalysis_cannot_restore_ejected_deck_metadata() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
        let generation = AtomicU64::new(0);
        apply_unload(
            &core,
            target(&core, DeckId::A, 0),
            &generation,
            &AtomicU64::new(0),
            false,
            false,
        )
        .unwrap();
        crate::analysis::reanalyze(&core, tracks[0], false).unwrap();
        let snapshot = core.engine.snapshot();
        let deck = &snapshot.decks[0];
        assert!(deck.track_id.is_none() && deck.cues.iter().all(Option::is_none));
        assert!(core.engine.deck_waveform(DeckId::A).is_none());
        assert!(core.analysis.loaded.lock().unwrap()[0].is_none());
    }

    #[test]
    fn unload_stops_the_running_auto_worker_without_repopulating_the_deck() {
        use std::time::{Duration, Instant};

        let (_dir, core, tracks) = crate::automix::tests::fixture();
        crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
        core.engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        let auto = Arc::new(AtomicU64::new(1));
        let (worker_core, worker_auto) = (core.clone(), auto.clone());
        let (tx, _rx) = channel();
        let worker = std::thread::spawn(move || {
            crate::automix::run(
                worker_core,
                tracks,
                Arc::new(false.into()),
                worker_auto,
                1,
                tx,
            );
        });
        let deadline = Instant::now() + Duration::from_secs(15);
        while !core.engine.snapshot().automix_on && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let staged = core.engine.snapshot().automix_on;
        let generation = AtomicU64::new(0);
        let request = target(&core, DeckId::B, 0);
        apply_unload(&core, request, &generation, &auto, false, true).unwrap();
        worker.join().unwrap();
        assert!(
            staged,
            "AUTO should have staged a real plan before unloading"
        );
        core.engine.render_offline(1024);
        let after = core.engine.snapshot();
        assert!(after.decks[1].track_id.is_none());
        assert!(!after.automix_on && after.decks[0].playing);
        assert_eq!(auto.load(Ordering::Acquire), 2);
    }
}
