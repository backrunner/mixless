use super::order_cache::Stamp;
use super::ordering::{OrderingJob, publish_order};
use super::*;

#[test]
fn selecting_best_before_the_first_queue_still_prepares_playlist_playback() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let (tx, _rx) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(tx).unwrap();
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(job);
    set_order_mode(&core, OrderMode::Best, tracks.clone());
    assert_eq!(core.mix_preparation.tracks(), tracks);
    assert_eq!(core.mix_preparation.queue_intent().mode, OrderMode::Order);
    assert_eq!(core.mix_preparation.queue_view().requested, OrderMode::Best);
}

#[test]
fn best_completion_waits_for_playback_ack_and_late_ack_cannot_override_a_new_choice() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let (tx, _rx) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(tx).unwrap();
    set_order_mode(&core, OrderMode::Order, tracks.clone());
    let order = core.mix_preparation.queue_intent();
    assert!(core.mix_preparation.publish_queue(order, entries(&tracks)));
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    set_order_mode(&core, OrderMode::Best, tracks.clone());
    let view = core.mix_preparation.queue_view();
    assert_eq!(view.requested, OrderMode::Best);
    assert_eq!(view.effective, OrderMode::Order);
    assert_eq!(*view.rows, entries(&tracks));
    assert!(view.pending);
    let best = vec![tracks[0], tracks[2], tracks[1]];
    assert!(publish_order(
        &core,
        &job,
        best.clone(),
        Some(Stamp::current(&core)),
        false
    ));
    let ready = core.mix_preparation.queue_intent();
    assert_eq!(core.mix_preparation.tracks(), best);
    assert_eq!(*core.mix_preparation.queue_view().rows, entries(&tracks));
    assert!(core.mix_preparation.publish_queue(ready, entries(&best)));
    assert_eq!(core.mix_preparation.queue_view().effective, OrderMode::Best);
    set_order_mode(&core, OrderMode::Shuffle, tracks.clone());
    assert!(!core.mix_preparation.publish_queue(ready, entries(&best)));
    let shuffle = core.mix_preparation.queue_intent();
    assert!(
        core.mix_preparation
            .publish_queue(shuffle, entries(&tracks))
    );
    let q = core.mix_preparation.queue_view();
    assert_eq!(q.effective, OrderMode::Shuffle);
    assert!(!q.pending);
}

#[test]
fn pending_best_retains_a_shuffle_queue_and_stop_rejects_its_completion() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let (tx, _rx) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(tx).unwrap();
    set_order_mode(&core, OrderMode::Shuffle, tracks.clone());
    let shuffled = vec![tracks[2], tracks[0], tracks[1]];
    let intent = core.mix_preparation.queue_intent();
    core.mix_preparation
        .publish_queue(intent, entries(&shuffled));
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    set_order_mode(&core, OrderMode::Best, tracks.clone());
    assert_eq!(core.mix_preparation.queue_intent(), intent);
    assert_eq!(
        core.mix_preparation.queue_view().effective,
        OrderMode::Shuffle
    );
    assert_eq!(*core.mix_preparation.queue_view().rows, entries(&shuffled));
    core.mix_preparation.end_queue_session();
    assert!(
        !core
            .mix_preparation
            .publish_queue(intent, entries(&shuffled))
    );
    assert!(publish_order(
        &core,
        &job,
        shuffled,
        Some(Stamp::current(&core)),
        false
    ));
    assert!(core.mix_preparation.queue_view().rows.is_empty());
    assert_eq!(
        core.mix_preparation.queue_view().requested,
        OrderMode::Order
    );
}

#[test]
fn queue_rows_keep_occurrences_and_do_not_rotate_with_each_playhead_change() {
    let prep = Preparation::default();
    let tracks = vec![TrackId(1), TrackId(2), TrackId(1), TrackId(3)];
    let rows = entries(&tracks);
    assert_ne!(rows[0], rows[2]);
    let intent = prep.queue_intent();
    prep.publish_queue(intent, rows.clone());
    let before = prep.queue_view().rows;
    let mut rotated = rows.clone();
    rotated.rotate_left(1);
    prep.publish_queue(intent, rotated);
    assert!(Arc::ptr_eq(&before, &prep.queue_view().rows));
    prep.sequence.lock().unwrap().queue.reset(OrderMode::Order);
    assert!(!prep.publish_queue(intent, rows));
}

#[gpui::test]
fn a_published_shuffle_preserves_scrolled_list_and_duplicate_selection(
    cx: &mut gpui::TestAppContext,
) {
    use gpui::{px, size};
    let (_dir, core, ids) = crate::automix::tests::fixture();
    let (tx, _rx) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(tx).unwrap();
    let tracks: Vec<_> = ids.iter().copied().cycle().take(60).collect();
    let selected = entries(&tracks)[25];
    set_order_mode(&core, OrderMode::Order, tracks.clone());
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), entries(&tracks));
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut s = crate::state::UiState::new(core.clone(), cx);
        s.tracks = Arc::new(
            tracks
                .iter()
                .map(|id| core.library.get_track(*id).unwrap())
                .collect(),
        );
        s.automix_active = true;
        s.automix_tracks = tracks.clone();
        s.track_sel = Some(selected.track.0);
        s.track_sel_entry = Some(selected);
        s
    });
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    view.update(cx, |s, cx| {
        s.track_scroll
            .scroll_to_item_strict(25, gpui::ScrollStrategy::Top);
        cx.notify();
    });
    cx.run_until_parked();
    let before = view.read_with(cx, |s, _| s.track_scroll.0.borrow().base_handle.offset());
    assert!(before.y < px(0.));
    set_order_mode(&core, OrderMode::Shuffle, tracks.clone());
    let mut order = crate::automix::order::TrackOrder::new(tracks.clone(), Some(ids[0]), 123);
    let outgoing = order.current_entry().unwrap();
    order.next(true);
    let rows = order.queue_rows(outgoing, true);
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), rows.clone());
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert_eq!(s.track_scroll.0.borrow().base_handle.offset(), before);
        assert_eq!(s.track_sel_entry, Some(selected));
        assert_eq!(s.shown_entries(&s.shown_tracks()), rows);
    });
    core.shutting_down.store(true, Ordering::Release);
}

#[gpui::test]
fn full_workspace_order_menu_keeps_playback_rows_until_ack_and_dismisses(
    cx: &mut gpui::TestAppContext,
) {
    use gpui::{px, size};
    let (_dir, core, ids) = crate::automix::tests::fixture();
    let (tx, _rx) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(tx).unwrap();
    let tracks = vec![ids[0], ids[1], ids[0], ids[2]];
    set_order_mode(&core, OrderMode::Order, tracks.clone());
    use mixless_protocol::{Command, DeckId};
    crate::automix::load(&core, DeckId::A, ids[0], || true).unwrap();
    core.engine
        .dispatch(Command::SetCrossfader { value: -1. })
        .unwrap();
    core.engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    let stage = |input| {
        crate::automix::load(&core, DeckId::B, input, || true).unwrap();
        let a = crate::analysis::prepare(&core, ids[0]).unwrap();
        let b = crate::analysis::prepare(&core, input).unwrap();
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
        core.engine
            .load_plan_on((*plan).clone(), DeckId::A)
            .unwrap();
        core.engine.dispatch(Command::PauseAutomix).unwrap();
        committed(&core, plan);
    };
    stage(ids[1]);
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), entries(&tracks));
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    let (view, cx) = cx.add_window_view(|_, cx| {
        let mut s = crate::state::UiState::new(core.clone(), cx);
        s.tracks = Arc::new(
            tracks
                .iter()
                .map(|id| core.library.get_track(*id).unwrap())
                .collect(),
        );
        s.automix_active = true;
        s.automix_tracks = tracks.clone();
        s.track_sel = Some(ids[0].0);
        s.track_sel_entry = Some(entries(&tracks)[2]);
        s
    });
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    let open = |cx: &mut gpui::VisualTestContext| {
        let button = cx.debug_bounds("automix-order").unwrap();
        cx.simulate_click(button.center(), Default::default());
        cx.run_until_parked();
        let panel = cx.debug_bounds("order-menu-panel").unwrap();
        assert!(panel.right() <= px(1172.) && panel.bottom() <= px(752.));
    };
    open(cx);
    cx.simulate_keystrokes("end enter");
    cx.run_until_parked();
    assert_eq!(core.mix_preparation.queue_view().requested, OrderMode::Best);
    view.read_with(cx, |s, _| {
        assert!(!s.order_menu.open);
        assert_eq!(
            s.shown_tracks().iter().map(|t| t.id).collect::<Vec<_>>(),
            tracks
        );
    });
    let best = vec![ids[0], ids[2], ids[0], ids[1]];
    assert!(publish_order(
        &core,
        &job,
        best.clone(),
        Some(Stamp::current(&core)),
        false
    ));
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert_eq!(s.automix_queue.effective, OrderMode::Order)
    });
    assert_eq!(core.engine.snapshot().decks[1].track_id, Some(ids[1]));
    stage(ids[2]);
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), entries(&best));
    view.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert_eq!(s.automix_queue.effective, OrderMode::Best);
        assert_eq!(s.snapshot.decks[1].track_id, Some(ids[2]));
        assert!(s.snapshot.automix_paused);
        assert_eq!(
            s.shown_tracks().iter().map(|t| t.id).collect::<Vec<_>>(),
            best
        );
        assert_eq!(s.track_sel_entry, Some(entries(&tracks)[2]));
    });
    open(cx);
    let quantize = core.engine.snapshot().quantize;
    let outside = cx.debug_bounds("quantize-toggle").unwrap().center();
    cx.simulate_click(outside, Default::default());
    cx.run_until_parked();
    assert!(!view.read_with(cx, |s, _| s.order_menu.open));
    assert_eq!(core.engine.snapshot().quantize, quantize);
    open(cx);
    cx.simulate_keystrokes("escape");
    cx.run_until_parked();
    assert!(!view.read_with(cx, |s, _| s.order_menu.open));
    view.update(cx, |s, cx| {
        s.stop_automix();
        cx.notify();
    });
    cx.run_until_parked();
    view.read_with(cx, |s, _| {
        assert_eq!(
            s.shown_tracks().iter().map(|t| t.id).collect::<Vec<_>>(),
            tracks
        )
    });
    core.shutting_down.store(true, Ordering::Release);
}
