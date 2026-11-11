use super::*;

fn pair(a: TrackId, b: TrackId, exit: f32) -> ScheduledPair {
    ScheduledPair {
        key: format!("{}-{}-{exit}", a.0, b.0),
        plan: Arc::new(MixPlan {
            summary: Some(mixless_protocol::MixPlanSummary {
                pair: (a, b),
                strategy: mixless_protocol::StrategyId::BassSwap,
                score: 1.,
                used_fallback: false,
                length_bars: 24,
            }),
            t_in_a: exit,
            t_out_a: exit + 20.,
            t_in_b: 5.,
            t_end_b: 25.,
            ..Default::default()
        }),
    }
}

#[test]
fn commit_keeps_display_but_invalidates_execution_and_rejects_obsolete_results() {
    let prep = Preparation::default();
    let tracks = [TrackId(1), TrackId(2), TrackId(3)];
    prep.sequence.lock().unwrap().select(&tracks);
    let first = pair(tracks[0], tracks[1], 60.);
    let second = pair(tracks[1], tracks[2], 60.);
    assert!(prep.publish_previews(0, vec![Some(first.clone()), Some(second.clone()), None]));
    let before = prep.previews();
    let changed = pair(tracks[0], tracks[1], 70.);
    {
        let mut sequence = prep.sequence.lock().unwrap();
        sequence.commit(&tracks, changed.plan.clone());
        prep.revision.store(1, Ordering::Release);
    }
    let during = prep.previews();
    for (old, shown) in before.plans.iter().zip(&during.plans) {
        if let (Some(old), Some(shown)) = (old, shown) {
            assert!(
                Arc::ptr_eq(old, shown),
                "keep the complete displayed forecast"
            );
        } else {
            assert!(old.is_none() && shown.is_none());
        }
    }
    assert!(Arc::ptr_eq(during.active.as_ref().unwrap(), &changed.plan));
    assert!(
        prep.scheduled(&second.key, 0.).is_none(),
        "old exits cannot execute"
    );
    let revision = prep.preview_revision.load(Ordering::Acquire);
    assert!(!prep.publish_previews(0, vec![Some(first), Some(second), None]));
    assert_eq!(prep.preview_revision.load(Ordering::Acquire), revision);
    let next = pair(tracks[1], tracks[2], 80.);
    assert!(prep.publish_previews(1, vec![Some(changed.clone()), Some(next.clone()), None]));
    let after = prep.previews();
    assert!(Arc::ptr_eq(after.plans[0].as_ref().unwrap(), &changed.plan));
    assert!(Arc::ptr_eq(after.plans[1].as_ref().unwrap(), &next.plan));
    assert!(Arc::ptr_eq(after.active.as_ref().unwrap(), &changed.plan));
    assert!(prep.scheduled(&next.key, 0.).is_some());
}

#[test]
fn reorder_retains_only_unchanged_neighbours_until_the_new_forecast_is_ready() {
    let prep = Preparation::default();
    let tracks = [TrackId(1), TrackId(2), TrackId(3), TrackId(4)];
    prep.sequence.lock().unwrap().select(&tracks);
    let first = pair(tracks[0], tracks[1], 60.);
    let second = pair(tracks[1], tracks[2], 60.);
    let third = pair(tracks[2], tracks[3], 60.);
    prep.publish_previews(
        0,
        vec![Some(first.clone()), Some(second), Some(third), None],
    );
    {
        let mut sequence = prep.sequence.lock().unwrap();
        sequence.commit(&tracks, first.plan.clone());
        sequence.select(&[tracks[3], tracks[0], tracks[1], tracks[2]]);
        assert!(sequence.pairs.iter().all(Option::is_none));
        assert_eq!(
            sequence.displayed.iter().flatten().count(),
            3,
            "rotation keeps edges"
        );
        sequence.select(&[tracks[0], tracks[1], tracks[3], tracks[2]]);
    }
    let retained = prep.previews();
    assert_eq!(retained.plans.iter().flatten().count(), 1);
    assert!(Arc::ptr_eq(
        retained.plans[0].as_ref().unwrap(),
        &first.plan
    ));
    assert!(Arc::ptr_eq(retained.active.as_ref().unwrap(), &first.plan));
    // Missing decisions in a completed forecast are intentional, for example
    // a manual OUT consumed by the entry; old markers must not survive forever.
    prep.publish_previews(0, vec![None; tracks.len()]);
    assert!(prep.previews().plans.iter().all(Option::is_none));
    prep.sequence.lock().unwrap().select(&[]);
    assert!(prep.previews().plans.is_empty());
}

#[gpui::test]
fn loading_and_delayed_ui_messages_do_not_blank_or_roll_back_committed_ranges(
    cx: &mut gpui::TestAppContext,
) {
    use gpui::AppContext;
    use mixless_protocol::DeckId;

    let (_dir, core, tracks) = crate::automix::tests::fixture();
    *core.mix_preparation.playlist.lock().unwrap() = tracks.clone();
    core.mix_preparation
        .sequence
        .lock()
        .unwrap()
        .select(&tracks);
    // Hold background work so all observations happen during replanning.
    let (background, _pending) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(background).unwrap();
    let first = pair(tracks[0], tracks[1], 60.);
    let second = pair(tracks[1], tracks[2], 60.);
    core.mix_preparation
        .publish_previews(0, vec![Some(first.clone()), Some(second), None]);
    let state = cx.new(|cx| {
        let mut s = crate::state::UiState::new(core.clone(), cx);
        s.tracks = Arc::new(
            tracks
                .iter()
                .map(|id| core.library.get_track(*id).unwrap())
                .collect(),
        );
        s.automix_active = true;
        s.automix_plan = Some((DeckId::A, first.plan.clone()));
        s
    });
    // A real commit can precede its UI message.
    let actual = pair(tracks[0], tracks[1], 70.);
    committed(&core, actual.plan.clone());
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), entries(&tracks));
    state.update(cx, |s, _| {
        s.automix_queue = core.mix_preparation.queue_view()
    });
    let (before, key) = state.read_with(cx, |s, _| {
        (
            s.library_mix_overlays(&s.tracks),
            s.library_render_key(1440., 2.),
        )
    });
    assert_eq!(before[0].outgoing, Some((70., 90.)));
    assert_eq!(before[1].incoming, Some((5., 25.)));
    assert_eq!(
        before[1].outgoing,
        Some((60., 80.)),
        "retain the suffix while rebuilding"
    );
    state.update(cx, |s, _| {
        // Preparing clears the toolbar plan while the next deck loads.
        s.automix_plan = None;
        s.automix_loading = true;
        assert_eq!(s.library_mix_overlays(&s.tracks), before);
        assert_eq!(s.library_render_key(1440., 2.), key);
    });
    state.update(cx, |s, _| {
        s.snapshot.automix_on = false;
        s.snapshot.automix_progress = 1.;
        s.snapshot.automix_modified = true;
        assert_eq!(
            s.library_mix_overlays(&s.tracks),
            before,
            "automatic deck resets between handoffs must not hide the forecast"
        );
        s.snapshot.automix_on = true;
        assert!(
            s.library_mix_overlays(&s.tracks)
                .iter()
                .all(|r| *r == Default::default()),
            "a user modification of an armed plan still invalidates its preview"
        );
        s.snapshot.automix_modified = false;
    });
    state.update(cx, |s, _| {
        s.automix_plan = Some((DeckId::A, first.plan));
        assert_eq!(
            s.library_mix_overlays(&s.tracks),
            before,
            "old UI messages cannot roll back a commit"
        );
        assert_eq!(s.library_render_key(1440., 2.), key);
    });
    // A shuffle replaces both ends of superseded edges, not just the two
    // active markers, so another row cannot retain the abandoned transition.
    let shuffled = pair(tracks[0], tracks[2], 75.);
    committed(&core, shuffled.plan);
    core.mix_preparation.publish_queue(
        core.mix_preparation.queue_intent(),
        entries(&[tracks[0], tracks[2], tracks[1]]),
    );
    state.update(cx, |s, _| {
        s.automix_queue = core.mix_preparation.queue_view();
        let rows = s.library_mix_overlays(&s.tracks);
        assert_eq!(rows[0].outgoing, Some((75., 95.)));
        assert_eq!(rows[2].incoming, Some((5., 25.)));
        assert_eq!(rows[1], Default::default());
        s.automix_active = false;
        assert!(
            s.library_mix_overlays(&s.tracks)
                .iter()
                .all(|r| *r == Default::default())
        );
    });
}

#[gpui::test]
fn completed_engine_handoff_and_next_deck_reset_keep_visible_cues(cx: &mut gpui::TestAppContext) {
    use gpui::AppContext;
    use mixless_protocol::{Command, DeckId, Polyline};
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    for (deck, id) in [(DeckId::A, tracks[0]), (DeckId::B, tracks[1])] {
        crate::automix::load(&core, deck, id, || true).unwrap();
    }
    *core.mix_preparation.playlist.lock().unwrap() = tracks.clone();
    core.mix_preparation
        .sequence
        .lock()
        .unwrap()
        .select(&tracks);
    let (background, _pending) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(background).unwrap();
    let mut plan = (*pair(tracks[0], tracks[1], 1.).plan).clone();
    plan.summary.as_mut().unwrap().length_bars = 4;
    plan.t_out_a = 3.;
    plan.t_in_b = 0.;
    plan.t_end_b = 2.;
    plan.clock.nodes = vec![(0., 0.), (4., 2.)];
    plan.outgoing_source.nodes = vec![(0., 1.), (4., 3.)];
    plan.incoming_source.nodes = vec![(0., 0.), (4., 2.)];
    plan.lanes.xfader.nodes = vec![(0., -1.), (4., 1.)];
    plan.lanes.gain_a.nodes = vec![(0., 0.), (3.99, 0.), (4., -96.)];
    plan.lanes.gain_b = Polyline::constant(0.);
    plan.lanes.rate_a = Polyline::constant(1.);
    plan.lanes.rate_b = Polyline::constant(1.);
    for lane in [
        &mut plan.lanes.eq_a.low,
        &mut plan.lanes.eq_a.mid,
        &mut plan.lanes.eq_a.high,
        &mut plan.lanes.eq_b.low,
        &mut plan.lanes.eq_b.mid,
        &mut plan.lanes.eq_b.high,
        &mut plan.lanes.pitch_a,
        &mut plan.lanes.pitch_b,
        &mut plan.lanes.fx_send_a,
        &mut plan.lanes.fx_send_b,
    ] {
        *lane = Polyline::constant(0.);
    }
    plan.lanes.filter_a.lp_hz = Polyline::constant(20_000.);
    plan.lanes.filter_b.lp_hz = Polyline::constant(20_000.);
    plan.lanes.filter_a.hp_hz = Polyline::constant(20.);
    plan.lanes.filter_b.hp_hz = Polyline::constant(20.);
    core.engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    core.engine.load_plan_on(plan.clone(), DeckId::A).unwrap();
    committed(&core, Arc::new(plan));
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), entries(&tracks));
    let state = cx.new(|cx| {
        let mut s = crate::state::UiState::new(core.clone(), cx);
        s.tracks = Arc::new(
            tracks
                .iter()
                .map(|id| core.library.get_track(*id).unwrap())
                .collect(),
        );
        s.automix_active = true;
        s.automix_queue = core.mix_preparation.queue_view();
        s.snapshot = core.engine.snapshot();
        s
    });
    let before = state.read_with(cx, |s, _| s.library_mix_overlays(&s.tracks));
    assert_eq!(before[0].outgoing, Some((1., 3.)));
    assert_eq!(before[1].incoming, Some((0., 2.)));
    core.engine.render_offline(4 * 48_000);
    let completed = core.engine.snapshot();
    assert!(!completed.automix_on && completed.automix_progress == 1.);
    crate::automix::reset_deck(&core, DeckId::A, || true).unwrap();
    assert!(core.engine.snapshot().automix_modified);
    state.update(cx, |s, _| {
        s.snapshot = core.engine.snapshot();
        s.automix_loading = true;
        s.automix_plan = None;
        assert_eq!(s.library_mix_overlays(&s.tracks), before);
    });
}

#[test]
fn advancing_the_forecast_keeps_the_completed_entrance_but_not_failed_future_edges() {
    let prep = Preparation::default();
    let tracks = [TrackId(1), TrackId(2), TrackId(3), TrackId(4)];
    prep.sequence.lock().unwrap().select(&tracks);
    let first = pair(tracks[0], tracks[1], 60.);
    let second = pair(tracks[1], tracks[2], 70.);
    let third = pair(tracks[2], tracks[3], 80.);
    prep.publish_previews(
        0,
        vec![Some(first.clone()), Some(second.clone()), Some(third), None],
    );
    {
        let mut s = prep.sequence.lock().unwrap();
        s.anchor = Some(second.plan.clone());
        s.anchor_entries = Some([entries(&tracks)[1], entries(&tracks)[2]]);
        let mut rows = entries(&tracks);
        rows.rotate_left(2);
        s.following = Some(rows);
    }
    // C -> D failed; A -> B was already completed and is deliberately skipped.
    prep.publish_previews(0, vec![None, None, None, Some(second)]);
    let s = prep.sequence.lock().unwrap();
    let shown = s.aligned_previews(&entries(&tracks));
    assert!(Arc::ptr_eq(shown[0].as_ref().unwrap(), &first.plan));
    assert!(shown[2].is_none());
    assert!(s.pairs[2].is_none(), "history is display-only");
}

#[test]
fn commits_before_the_first_forecast_preserve_the_exact_completed_occurrence() {
    let prep = Preparation::default();
    let tracks = [TrackId(1), TrackId(2), TrackId(1), TrackId(3)];
    let rows = entries(&tracks);
    prep.sequence.lock().unwrap().select(&tracks);
    let first = pair(tracks[0], tracks[1], 60.);
    prep.sequence
        .lock()
        .unwrap()
        .commit(&tracks, first.plan.clone());
    assert!(prep.publish_queue(prep.queue_intent(), rows.clone()));
    let second = pair(tracks[1], tracks[2], 70.);
    prep.sequence
        .lock()
        .unwrap()
        .commit(&tracks, second.plan.clone());
    let mut advanced = rows.clone();
    advanced.rotate_left(1);
    assert!(prep.publish_queue(prep.queue_intent(), advanced));
    assert!(prep.publish_previews(0, vec![None, Some(second.clone()), None, None]));
    let shown = prep.queue_view();
    assert_eq!(*shown.rows, rows);
    assert_eq!(shown.pair, Some([rows[1], rows[2]]));
    assert!(Arc::ptr_eq(
        shown.preview.plans[0].as_ref().unwrap(),
        &first.plan
    ));
    assert!(Arc::ptr_eq(
        shown.preview.plans[1].as_ref().unwrap(),
        &second.plan
    ));
    assert!(shown.preview.plans[2].is_none());
}
