use super::*;
use crate::automix::AutomixMsg;

#[gpui::test]
fn disconnected_worker_recovers_loaded_next_deck_after_eof(cx: &mut gpui::TestAppContext) {
    use std::time::{Duration, Instant};
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
    crate::analysis::load(&core, DeckId::B, tracks[1], || true).unwrap();
    core.engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    core.engine.render_offline(8 * 48_000);
    assert!(!core.engine.snapshot().decks[0].playing);
    let state = cx.new(|cx| UiState::new(core.clone(), cx));
    let (tx, rx) = channel();
    tx.send(AutomixMsg::Preparing("Next track".into())).unwrap();
    drop(tx);
    state.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_tracks = tracks.clone();
        s.automix_rx = Some(rx);
        s.poll_automix();
        assert!(s.automix_retry.is_some());
        s.automix_retry = Some(Instant::now());
        s.poll_automix();
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    while !core.engine.snapshot().automix_on && Instant::now() < deadline {
        core.engine.render_offline(480);
        state.update(cx, |s, _| {
            s.snapshot = core.engine.snapshot();
            s.poll_automix();
        });
        std::thread::sleep(Duration::from_millis(10));
    }
    let armed = core.engine.snapshot().automix_on;
    let pair = core.engine.automix_summary().map(|s| s.pair);
    // Keep this committed handoff, but prevent the recovered host from
    // immediately arming the following one and resetting progress to zero.
    state.update(cx, |s, _| {
        s.automix_epoch.fetch_add(1, Ordering::AcqRel);
    });
    core.engine.render_offline(10 * 48_000);
    let completed = core.engine.snapshot().automix_progress;
    state.update(cx, |s, _| s.stop_automix());
    assert!(armed, "AUTO did not recover from the dead worker");
    assert_eq!(
        pair,
        Some((tracks[1], tracks[2])),
        "resume the already loaded next deck"
    );
    assert_eq!(completed, 1.);
}

#[gpui::test]
fn playback_status_finishes_loading_without_a_transition_plan(cx: &mut gpui::TestAppContext) {
    let (_dir, core, _) = crate::automix::tests::fixture();
    let state = cx.new(|cx| UiState::new(core.clone(), cx));
    let (tx, rx) = channel();
    state.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_rx = Some(rx);
        tx.send(AutomixMsg::Preparing("First track".into()))
            .unwrap();
        s.poll_automix();
        assert!(s.automix_loading);
        tx.send(AutomixMsg::Status("Starting First track".into()))
            .unwrap();
        s.poll_automix();
        assert_eq!(s.automix_status, "Starting First track");
        assert!(
            !s.automix_loading,
            "playback started but the spinner remains"
        );
        assert!(s.automix_rx.is_some());
        assert!(s.automix_retry.is_none());
    });
}

#[gpui::test]
fn disconnected_worker_exits_loading_and_schedules_recovery(cx: &mut gpui::TestAppContext) {
    let (_dir, core, _) = crate::automix::tests::fixture();
    let state = cx.new(|cx| UiState::new(core.clone(), cx));
    let (tx, rx) = channel();
    tx.send(AutomixMsg::Preparing("Next track".into())).unwrap();
    drop(tx); // A worker can terminate without sending Done (for example, a panic).
    state.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_rx = Some(rx);
        assert!(s.poll_automix());
        assert!(
            !s.automix_loading,
            "a closed channel must not leave AUTO loading forever"
        );
        assert!(s.automix_rx.is_none());
        assert!(s.automix_retry.is_some());
        assert!(s.automix_active);
        let retry = s.automix_retry;
        s.poll_automix();
        assert_eq!(s.automix_retry, retry, "polling must not postpone recovery");
    });
}

#[gpui::test]
fn loaded_track_reports_planning_then_ready_without_waiting_for_another_status(
    cx: &mut gpui::TestAppContext,
) {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let a = crate::analysis::prepare(&core, tracks[0]).unwrap();
    let b = crate::analysis::prepare(&core, tracks[1]).unwrap();
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
    let state = cx.new(|cx| UiState::new(core.clone(), cx));
    let (tx, rx) = channel();
    state.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_rx = Some(rx);
        tx.send(AutomixMsg::Preparing("Next track".into())).unwrap();
        s.poll_automix();
        assert_eq!(s.automix_status, "Loading Next track");
        tx.send(AutomixMsg::Planning("Next track".into())).unwrap();
        s.poll_automix();
        assert_eq!(s.automix_status, "Planning transition to Next track");
        assert!(s.automix_loading, "planning is still in progress");
        assert!(
            s.automix_plan.is_none(),
            "loaded audio alone is not an armed plan"
        );
        tx.send(AutomixMsg::Plan(DeckId::A, plan, "Next track".into()))
            .unwrap();
        s.poll_automix();
        assert_eq!(s.automix_status, "Next: Next track");
        assert!(!s.automix_loading);
        tx.send(AutomixMsg::Done(Ok(()))).unwrap();
        drop(tx);
        s.poll_automix();
        assert_eq!(
            s.automix_status, "Waiting for playback",
            "Done must win over disconnection"
        );
    });
}
