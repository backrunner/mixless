//! Prepare changed queue decisions while the original handoff stays armed.
use super::*;
use std::sync::mpsc::{Receiver, channel};

const REPLACE_LEAD: f32 = 2.;

pub(super) struct Update {
    revision: u64,
    tracks: Vec<TrackId>,
    pub order: order::TrackOrder,
    candidate: TrackId,
    prepared: Option<(
        crate::analysis::ReadyLoad,
        crate::analysis::PreparedTrack,
        Arc<mixless_protocol::MixPlan>,
        mixless_engine::PreparedMix,
    )>,
    sources: [u64; 2],
    sample_rate: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    fn armed(
        core: &Arc<AppCore>,
        ids: &[TrackId],
    ) -> (Arc<mixless_protocol::MixPlan>, order::TrackOrder) {
        load(core, DeckId::A, ids[0], || true).unwrap();
        load(core, DeckId::B, ids[1], || true).unwrap();
        command(core, Command::SetCrossfader { value: -1. }).unwrap();
        command(core, Command::PlayPause { deck: DeckId::A }).unwrap();
        let a = crate::analysis::prepare(core, ids[0]).unwrap();
        let b = crate::analysis::prepare(core, ids[1]).unwrap();
        let plan = Arc::new(mixless_mixplan::short_handoff(
            &mixless_mixplan::PlanContext {
                outgoing: &a.analysis,
                incoming: &b.analysis,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            },
            0.,
        ));
        assert!(plan.t_in_a > REPLACE_LEAD, "{plan:?}");
        core.engine
            .load_plan_on((*plan).clone(), DeckId::A)
            .unwrap();
        preparation::committed(core, plan.clone());
        let mut order = order::TrackOrder::new(ids.to_vec(), Some(ids[0]), 1);
        assert_eq!(order.next(false), ids[1]);
        (plan, order)
    }
    fn pending(core: &Arc<AppCore>, watch: &mut Watch, order: &order::TrackOrder) -> Update {
        let end = std::time::Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(update) = watch.poll(core, DeckId::A, 0., order, false) {
                if update.prepared.is_some() || watch.pending.is_none() {
                    return update;
                }
            }
            assert!(std::time::Instant::now() < end, "no revised candidate");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    #[ignore = "uses an explicit real-library backup"]
    fn audit_cross_playlist_replacement_keeps_the_selected_entry() {
        let db = std::env::var("MIXLESS_SET_AUDIT_DB").expect("SQLite backup");
        let (_dir, mut core, _) = super::super::tests::fixture();
        Arc::get_mut(&mut core).unwrap().library =
            mixless_library::Library::open(std::path::Path::new(&db)).unwrap();
        if let Ok(cache) = std::env::var("MIXLESS_SET_AUDIT_STEMS") {
            Arc::get_mut(&mut core).unwrap().stems = Some(mixless_stems::Processor::new(
                _dir.path().join("models"),
                cache.into(),
                false,
            ));
        }
        let source = core
            .library
            .playlist_tracks(mixless_protocol::PlaylistId(4))
            .unwrap();
        let target = core
            .library
            .playlist_tracks(mixless_protocol::PlaylistId(3))
            .unwrap();
        let ids: Vec<_> = source.iter().map(|t| t.id).collect();
        prepare_playlist(&core, ids.clone());
        let (old, order) = armed(&core, &ids);
        core.engine.render_offline(20 * 48_000);
        let mut watch = Watch::new(core.mix_preparation.revision());
        prepare_playlist(&core, target.iter().map(|t| t.id).collect());
        let update = pending(&core, &mut watch, &order);
        assert!(update.prepared.is_some());
        let next = update
            .publish(&core, DeckId::A, &old, || true)
            .unwrap()
            .unwrap();
        let plan = next.transition.unwrap();
        assert!(
            plan.t_in_b > 1.,
            "fading introduction chosen again: {}",
            plan.t_in_b
        );
        core.engine.render_offline(256);
        let snapshot = core.engine.snapshot();
        let b = &snapshot.decks[1];
        assert_eq!(b.track_id, Some(target[0].id));
        assert!(!b.playing);
        assert!(snapshot.decks[0].playing);
        let planned_frame = (plan.t_in_b * b.src_sample_rate as f32).round() as u64;
        assert!(
            b.frame.abs_diff(planned_frame) <= 1,
            "loaded at {} instead of {}",
            b.frame,
            planned_frame
        );
        let now = snapshot.decks[0].frame as f32 / snapshot.decks[0].src_sample_rate as f32;
        let audio = core
            .engine
            .render_offline(((plan.t_out_a - now + 2.) * 48_000.) as usize);
        assert!(audio.iter().all(|s| s.is_finite() && s.abs() <= 1.));
        let done = core.engine.snapshot();
        assert_eq!(done.automix_progress, 1.);
        assert!(done.decks[1].playing);
        assert!(done.decks[1].frame as f32 / done.decks[1].src_sample_rate as f32 >= plan.t_end_b);
        println!(
            "Replacement handoff completed, finite PCM, peak {:.5}",
            audio.iter().map(|s| s.abs()).fold(0., f32::max)
        );
        println!(
            "{} -> {}: entry {:.3}s; staged frame {} at {} Hz; outgoing continues",
            source[0].title, target[0].title, plan.t_in_b, b.frame, b.src_sample_rate
        );
    }

    #[test]
    fn queue_intent_survives_a_handoff_that_finishes_before_preparation() {
        let (_dir, core, ids) = super::super::tests::fixture();
        prepare_playlist(&core, ids.clone());
        let (_old, mut order) = armed(&core, &ids);
        let mut watch = Watch::new(core.mix_preparation.revision());
        prepare_playlist(&core, vec![ids[0], ids[2], ids[1]]);
        let intent = watch.poll(&core, DeckId::A, 0., &order, false).unwrap();
        assert!(intent.prepared.is_none());
        intent.defer(&mut order);
        drop(watch);
        assert_eq!(order.next(false), ids[2]);
        assert_eq!(order.next(false), ids[0]);
    }
    #[test]
    fn changed_playlist_replaces_only_silent_deck_after_preparation_and_completes() {
        let (_dir, core, ids) = super::super::tests::fixture();
        prepare_playlist(&core, ids.clone());
        let (old, order) = armed(&core, &ids);
        let mut watch = Watch::new(core.mix_preparation.revision());
        prepare_playlist(&core, vec![ids[2], ids[1]]);
        let update = pending(&core, &mut watch, &order);
        assert_eq!(update.incoming(), ids[2]);
        assert!(update.prepared.is_some());
        let before = core.engine.snapshot();
        assert_eq!(
            before.decks[1].track_id,
            Some(ids[1]),
            "preparation leaves old candidate armed"
        );
        let next = update
            .publish(&core, DeckId::A, &old, || true)
            .unwrap()
            .unwrap();
        let after = core.engine.snapshot();
        assert_eq!(after.decks[0].frame, before.decks[0].frame);
        assert_eq!(after.decks[0].playing, before.decks[0].playing);
        assert_eq!(after.decks[1].track_id, Some(ids[2]));
        assert!(after.automix_on && !after.decks[1].playing);
        let plan = next.transition.unwrap();
        core.engine
            .render_offline(((plan.t_in_a + plan.duration_sec() + 0.1) * 48000.) as usize);
        let done = core.engine.snapshot();
        assert_eq!(done.automix_progress, 1.);
        assert!(done.decks[1].playing);
    }
    #[test]
    fn late_reorder_keeps_both_sources_and_defers_new_candidate_exactly_once() {
        for delta in [-0.5, 0.5] {
            let (_dir, core, ids) = super::super::tests::fixture();
            prepare_playlist(&core, ids.clone());
            let (old, mut order) = armed(&core, &ids);
            let mut watch = Watch::new(core.mix_preparation.revision());
            prepare_playlist(&core, vec![ids[0], ids[2], ids[1]]);
            let update = pending(&core, &mut watch, &order);
            assert!(update.prepared.is_some());
            core.engine
                .render_offline(((old.t_in_a + delta) * 48000.) as usize);
            let before = core.engine.snapshot();
            update.defer(&mut order);
            assert!(
                update
                    .publish(&core, DeckId::A, &old, || true)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(core.engine.snapshot(), before);
            assert_eq!(order.next(false), ids[2]);
            assert_eq!(order.next(false), ids[0]);
        }
    }
    #[test]
    fn obsolete_evidence_and_cancelled_session_cannot_publish_or_unload() {
        for cancel in [false, true] {
            let (_dir, core, ids) = super::super::tests::fixture();
            prepare_playlist(&core, ids.clone());
            let (old, order) = armed(&core, &ids);
            let mut watch = Watch::new(core.mix_preparation.revision());
            prepare_playlist(&core, vec![ids[2], ids[1]]);
            let update = pending(&core, &mut watch, &order);
            assert!(update.prepared.is_some());
            if !cancel {
                refresh_previews(&core);
            }
            let before = core.engine.snapshot();
            assert!(
                update
                    .publish(&core, DeckId::A, &old, || !cancel)
                    .unwrap()
                    .is_none()
            );
            assert_eq!(core.engine.snapshot(), before);
        }
    }
    #[test]
    fn same_order_evidence_refresh_preserves_candidate_and_future_order() {
        let (_dir, core, ids) = super::super::tests::fixture();
        prepare_playlist(&core, ids.clone());
        let (_old, order) = armed(&core, &ids);
        let mut watch = Watch::new(core.mix_preparation.revision());
        refresh_previews(&core);
        let update = pending(&core, &mut watch, &order);
        assert_eq!(update.incoming(), ids[1]);
        assert_eq!(update.order.preview(3, false), order.preview(3, false));
    }
}

pub(super) struct Watch {
    seen: u64,
    pending: Option<Receiver<Option<Update>>>,
}

impl Watch {
    pub fn new(seen: u64) -> Self {
        Self {
            seen,
            pending: None,
        }
    }

    pub fn poll(
        &mut self,
        core: &Arc<AppCore>,
        outgoing: DeckId,
        entry: f32,
        order: &order::TrackOrder,
        shuffle: bool,
    ) -> Option<Update> {
        let revision = core.mix_preparation.revision();
        if revision != self.seen {
            // Obsolete preparation must not delay a new queue intent. Its
            // worker checks revision before each expensive dependent step.
            self.pending = None;
        }
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    if let Some(update) =
                        result.filter(|u| u.revision == core.mix_preparation.revision())
                    {
                        return Some(update);
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => return None,
                Err(_) => self.pending = None,
            }
        }
        if revision == self.seen {
            return None;
        }
        self.seen = revision;
        let tracks = core.mix_preparation.tracks();
        if tracks.is_empty() {
            return None;
        }
        let snapshot = core.engine.snapshot();
        let a = snapshot.deck(outgoing);
        let Some(id) = a.track_id else { return None };
        let mut proposal = order.clone();
        let candidate = if order.matches(&tracks) && !order.has_deferred() {
            // Evidence completion must not draw a fresh random successor.
            snapshot.decks[1 - outgoing.index()].track_id?
        } else {
            proposal.rebase(tracks.clone(), id);
            proposal.next(shuffle)
        };
        let following = proposal.preview(1, shuffle)[0];
        let core = core.clone();
        let sources = [DeckId::A, DeckId::B].map(|d| core.engine.deck_load_revision(d));
        let sample_rate = core.engine.sample_rate();
        let (tx, rx) = channel();
        self.pending = Some(rx);
        // Record the new candidate's next-slot priority immediately, even if
        // decode finishes only after this handoff and this Watch is dropped.
        let intent = Update {
            revision,
            tracks: tracks.clone(),
            order: proposal.clone(),
            candidate,
            prepared: None,
            sources,
            sample_rate,
        };
        std::thread::spawn(move || {
            let prepared = (|| {
                if core.mix_preparation.revision() != revision {
                    return None;
                }
                let a = crate::analysis::prepare(&core, id).ok()?;
                if core.mix_preparation.revision() != revision {
                    return None;
                }
                let ready = crate::analysis::ready_load(&core, candidate).ok()?;
                if core.mix_preparation.revision() != revision {
                    return None;
                }
                let snapshot = core.engine.snapshot();
                let d = snapshot.deck(outgoing);
                if d.track_id != Some(id) {
                    return None;
                }
                let earliest =
                    d.frame as f32 / d.src_sample_rate.max(1) as f32 + REPLACE_LEAD * d.rate;
                let plan = pair::scheduled_pair(
                    &core,
                    &a,
                    &ready.prepared,
                    earliest,
                    entry,
                    offset(d),
                    PerformanceOffset::identity(),
                    Some(following),
                    core.engine.four_stems_ready(outgoing) && ready.four_stems_ready(),
                )
                .ok()?
                .plan;
                if core.mix_preparation.revision() != revision {
                    return None;
                }
                let dense = core
                    .engine
                    .prepare_replacement(
                        (*plan).clone(),
                        outgoing,
                        ready.buf.clone(),
                        ready.stems.clone(),
                    )
                    .ok()?;
                Some((ready, a, plan, dense))
            })();
            let _ = tx.send(Some(Update {
                revision,
                tracks,
                order: proposal,
                candidate,
                prepared,
                sources,
                sample_rate,
            }));
        });
        Some(intent)
    }
}

impl Update {
    pub fn incoming(&self) -> TrackId {
        self.candidate
    }

    pub fn unchanged(&self, plan: &mixless_protocol::MixPlan) -> bool {
        self.prepared.as_ref().is_some_and(|(_, _, proposed, _)| {
            serde_json::to_value(proposed.as_ref()).ok() == serde_json::to_value(plan).ok()
        })
    }

    pub fn defer(&self, order: &mut order::TrackOrder) {
        order.replace(self.tracks.clone());
        order.defer(self.incoming());
    }

    pub fn publish(
        self,
        core: &Arc<AppCore>,
        outgoing: DeckId,
        old: &mixless_protocol::MixPlan,
        active: impl Fn() -> bool,
    ) -> Result<Option<next::Next>, String> {
        let incoming = if outgoing == DeckId::A {
            DeckId::B
        } else {
            DeckId::A
        };
        let Some((ready, a, plan, dense)) = self.prepared else {
            return Ok(None);
        };
        let b = ready.prepared.clone();
        let expected = old.summary.as_ref().ok_or("Missing armed pair")?.pair;
        let accepted = crate::analysis::publish_load(core, incoming, ready, || {
            if !active() || core.mix_preparation.revision() != self.revision {
                return false;
            }
            if core.engine.sample_rate() != self.sample_rate
                || [DeckId::A, DeckId::B].map(|d| core.engine.deck_load_revision(d)) != self.sources
            {
                return false;
            }
            let snap = core.engine.snapshot();
            let d = snap.deck(outgoing);
            let now = d.frame as f32 / d.src_sample_rate.max(1) as f32;
            now + REPLACE_LEAD * d.rate < plan.t_in_a
                && core
                    .engine
                    .cancel_armed_plan(outgoing, expected, REPLACE_LEAD)
        })?;
        if !accepted {
            return Ok(None);
        }
        reset_deck(core, incoming, &active)?;
        // The immutable envelope was compiled while the previous plan remained
        // armed. Publication performs no decode, stem reads or gain analysis.
        let transition = next::prepare_and_publish(core, outgoing, &active, || {
            Ok(Some((plan.clone(), dense.clone())))
        })?;
        Ok(Some(next::Next {
            outgoing: a,
            incoming: b,
            transition,
        }))
    }
}
