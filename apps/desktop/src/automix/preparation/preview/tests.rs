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
    assert_eq!(retained.plans.len(), 1);
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
    state.update(cx, |s, _| {
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
