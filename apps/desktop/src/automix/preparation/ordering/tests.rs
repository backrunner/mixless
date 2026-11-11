use super::*;

#[test]
fn leaving_best_does_not_wait_for_the_order_result_cache() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    job.report(42, 100);
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    core.mix_preparation.best.store(true, Ordering::Release);
    core.mix_preparation.warming.store(true, Ordering::Release);
    let cache = core.mix_preparation.arranged.lock().unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker_core = core.clone();
    let worker_tracks = tracks.clone();
    let worker = std::thread::spawn(move || {
        set_best(&worker_core, false, worker_tracks);
        tx.send(()).unwrap();
    });
    let switched = rx.recv_timeout(std::time::Duration::from_secs(1));
    // Always release the simulated busy cache and join before asserting.
    drop(cache);
    worker.join().unwrap();
    assert!(switched.is_ok(), "ORDER waited for the Best result cache");
    assert!(!core.mix_preparation.best());
    assert!(order_ready(&core, &tracks));
    assert_eq!(core.mix_preparation.tracks(), tracks);
    assert!(Arc::ptr_eq(
        core.mix_preparation
            .arranging
            .lock()
            .unwrap()
            .as_ref()
            .unwrap(),
        &job
    ));
}

#[test]
fn finishing_best_after_a_mode_change_only_warms_the_cache() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    core.mix_preparation.best.store(true, Ordering::Release);
    core.mix_preparation
        .defaulting
        .store(true, Ordering::Release);
    set_best(&core, false, tracks.clone());
    let expected = vec![tracks[2], tracks[0], tracks[1]];
    assert!(publish_order(
        &core,
        &job,
        expected.clone(),
        Some(Stamp::current(&core)),
        true
    ));
    assert!(!core.mix_preparation.best());
    assert_eq!(core.mix_preparation.tracks(), tracks);
    assert!(arranged(&core, &tracks).is_none());
    assert!(order_ready(&core, &tracks));
    // The completed work is available if the user explicitly returns to Best.
    set_best(&core, true, tracks.clone());
    assert_eq!(core.mix_preparation.tracks(), expected);
}

#[test]
fn an_old_best_result_cannot_publish_after_a_source_switch() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    set_best(&core, false, tracks.clone());
    reset_order_for_playlist(&core, None);
    assert!(!publish_order(
        &core,
        &job,
        tracks.clone(),
        Some(Stamp::current(&core)),
        true
    ));
    assert!(!core.mix_preparation.best());
    assert!(core.mix_preparation.tracks().is_empty());
    assert!(core.mix_preparation.arranged.lock().unwrap().is_none());
}

#[gpui::test]
fn best_progress_keeps_order_and_shuffle_clickable(cx: &mut gpui::TestAppContext) {
    use crate::state::UiState;
    use gpui::{AppContext, IntoElement, Render, prelude::*, px};
    struct Header(gpui::Entity<UiState>);
    impl Render for Header {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            self.0.update(cx, |s, cx| {
                gpui::div()
                    .relative()
                    .size_full()
                    .child(s.render_topbar(window.viewport_size().width.into(), cx))
                    .children(if s.order_menu.open {
                        s.render_order_menu(window, cx)
                    } else {
                        None
                    })
            })
        }
    }
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    job.report(42, 100);
    let state = cx.new(|cx| {
        let mut state = UiState::new(core.clone(), cx);
        state.tracks = Arc::new(
            tracks
                .iter()
                .map(|id| core.library.get_track(*id).unwrap())
                .collect(),
        );
        // Enter AUTO through its public actions so the retained source is set.
        // Shuffle opts out of the default cache lookup before the controlled
        // in-progress Best job below is installed.
        state.cycle_automix_order();
        state.toggle_automix();
        state.automix_shuffle.store(false, Ordering::Release);
        state.automix_loading = true;
        state.automix_status = "Finding the best order".into();
        state.automix_preparation_progress = Some(0.42);
        state
    });
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    core.mix_preparation.best.store(true, Ordering::Release);
    core.mix_preparation.request_mode(OrderMode::Best);
    core.mix_preparation
        .defaulting
        .store(true, Ordering::Release);
    let (header, cx) = cx.add_window_view(|_, _| Header(state.clone()));
    cx.simulate_resize(gpui::size(px(1440.), px(300.)));
    cx.run_until_parked();
    let button = cx.debug_bounds("automix-order").unwrap();
    cx.simulate_click(button.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let entry = cx.debug_bounds("order-menu-entry-0").unwrap().center();
    cx.simulate_click(entry, Default::default());
    state.read_with(cx, |s, _| {
        assert!(!core.mix_preparation.best());
        assert!(!s.automix_shuffle.load(Ordering::Acquire));
    });
    header.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    let button = cx.debug_bounds("automix-order").unwrap();
    cx.simulate_click(button.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let entry = cx.debug_bounds("order-menu-entry-1").unwrap().center();
    cx.simulate_click(entry, Default::default());
    let expected = vec![tracks[2], tracks[0], tracks[1]];
    assert!(publish_order(
        &core,
        &job,
        expected,
        Some(Stamp::current(&core)),
        true
    ));
    state.update(cx, |s, _| {
        assert!(s.automix_shuffle.load(Ordering::Acquire));
        assert!(!core.mix_preparation.best());
        assert_eq!(core.mix_preparation.tracks(), tracks);
        s.stop_automix();
    });
    core.shutting_down.store(true, Ordering::Release);
}

#[test]
fn automatic_start_loads_a_persisted_best_without_running_the_optimizer() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let prepared: Vec<_> = tracks
        .iter()
        .map(|id| crate::analysis::prepare(&core, *id).unwrap())
        .collect();
    let options = mixless_mixplan::PlannerOptions {
        harmonic_key_shift: true,
        live_moves: core.settings.get().live_moves,
        ..Default::default()
    };
    let key = crate::automix::preparation::order_cache::key(&prepared, &options).unwrap();
    core.library.save_best_order(&key, &[2, 0, 1]).unwrap();
    default_to_best(&core, tracks.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !order_ready(&core, &tracks) {
        assert!(
            std::time::Instant::now() < deadline,
            "default cache lookup stalled"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(core.mix_preparation.best());
    assert_eq!(
        arranged(&core, &tracks),
        Some(vec![tracks[2], tracks[0], tracks[1]])
    );
    assert_eq!(
        core.mix_preparation.tracks(),
        vec![tracks[2], tracks[0], tracks[1]]
    );
}

#[test]
fn cache_miss_starts_in_order_and_background_completion_only_warms_next_start() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    // Keep the lookup worker under test control so the optimizer cannot race
    // startup. A disk miss is already known, but no order is available yet.
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    job.lookup_done.store(true, Ordering::Release);
    *core.mix_preparation.arranging.lock().unwrap() = Some(job);
    default_to_best(&core, tracks.clone());
    assert!(order_ready(&core, &tracks));
    assert!(!core.mix_preparation.best());
    assert_eq!(core.mix_preparation.tracks(), tracks);
    assert_eq!(arranged(&core, &tracks), None);

    let expected = vec![tracks[2], tracks[0], tracks[1]];
    *core.mix_preparation.arranged.lock().unwrap() = Some(Arranged {
        input: tracks.clone(),
        order: expected.clone(),
        stamp: Some(Stamp::current(&core)),
    });
    *core.mix_preparation.arranging.lock().unwrap() = None;
    assert_eq!(arrange(&core, tracks.clone()), tracks);
    assert!(
        !core.mix_preparation.best(),
        "completing Best must not opt in this session"
    );
    assert_eq!(arranged(&core, &tracks), None);
    assert_eq!(core.mix_preparation.tracks(), tracks);

    default_to_best(&core, tracks.clone());
    assert!(order_ready(&core, &tracks));
    assert!(core.mix_preparation.best());
    assert_eq!(arranged(&core, &tracks), Some(expected.clone()));
    assert_eq!(core.mix_preparation.tracks(), expected);
}

#[test]
fn manual_best_uses_ready_cache_and_explicit_order_survives_next_start() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let expected = vec![tracks[1], tracks[2], tracks[0]];
    *core.mix_preparation.arranged.lock().unwrap() = Some(Arranged {
        input: tracks.clone(),
        order: expected.clone(),
        stamp: Some(Stamp::current(&core)),
    });
    set_best(&core, true, tracks.clone());
    assert_eq!(core.mix_preparation.tracks(), expected);
    set_best(&core, false, tracks.clone());
    default_to_best(&core, tracks.clone());
    assert!(order_ready(&core, &tracks));
    assert!(!core.mix_preparation.best());
    assert_eq!(core.mix_preparation.tracks(), tracks);
}

#[test]
fn completed_orders_are_reused_after_switching_playlists_and_clearing_memory() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let prepared: Vec<_> = tracks
        .iter()
        .map(|id| crate::analysis::prepare(&core, *id).unwrap())
        .collect();
    let options = mixless_mixplan::PlannerOptions {
        harmonic_key_shift: true,
        live_moves: core.settings.get().live_moves,
        ..Default::default()
    };
    let key = crate::automix::preparation::order_cache::key(&prepared, &options).unwrap();
    // A recognizable persisted permutation proves the worker takes the hit
    // instead of running the optimizer and producing its own answer.
    core.library.save_best_order(&key, &[2, 0, 1]).unwrap();
    core.mix_preparation.best.store(true, Ordering::Release);
    let expected = vec![tracks[2], tracks[0], tracks[1]];
    let wait = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while core.mix_preparation.arranging.lock().unwrap().is_some() {
            assert!(
                std::time::Instant::now() < deadline,
                "ordering did not finish"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    };
    for input in [
        tracks.clone(),
        vec![tracks[1], tracks[2], tracks[0]],
        tracks.clone(),
    ] {
        *core.mix_preparation.arranged.lock().unwrap() = None;
        arrange(&core, input.clone());
        wait();
        assert!(order_ready(&core, &input));
        if input == tracks {
            assert_eq!(arranged(&core, &tracks), Some(expected.clone()));
        }
    }
    core.analysis
        .content_revision
        .fetch_add(1, Ordering::Release);
    let cached = core.mix_preparation.arranged.lock().unwrap();
    assert!(cached.as_ref().unwrap().stamp != Some(Stamp::current(&core)));
}

#[test]
fn progress_is_monotonic_and_belongs_to_its_playlist_job() {
    let preparation = Preparation::default();
    let playlist = vec![TrackId(1), TrackId(2), TrackId(3)];
    let job = Arc::new(OrderingJob::new(playlist.clone()));
    *preparation.arranging.lock().unwrap() = Some(job.clone());
    assert_eq!(preparation.order_progress(&playlist), None);
    job.report(40, 100);
    job.report(20, 100);
    assert_eq!(preparation.order_progress(&playlist), Some(0.4));
    assert_eq!(preparation.order_progress(&[TrackId(4)]), None);
    let replacement = Arc::new(OrderingJob::new(playlist.clone()));
    *preparation.arranging.lock().unwrap() = Some(replacement.clone());
    job.report(100, 100);
    assert_eq!(preparation.order_progress(&playlist), None);
    replacement.report(10, 100);
    assert_eq!(preparation.order_progress(&playlist), Some(0.1));
    *preparation.arranging.lock().unwrap() = None;
    assert_eq!(preparation.order_progress(&playlist), None);
}

#[test]
fn a_failed_best_worker_keeps_the_applied_queue_and_can_be_retried() {
    let (_dir, core, tracks) = crate::automix::tests::fixture();
    let (tx, _rx) = std::sync::mpsc::channel();
    core.mix_preparation.sender.set(tx).unwrap();
    set_order_mode(&core, OrderMode::Order, tracks.clone());
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), entries(&tracks));
    let job = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(job.clone());
    set_order_mode(&core, OrderMode::Best, tracks.clone());
    let failed_core = core.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = WorkerGuard {
                core: failed_core,
                job,
            };
            panic!("controlled optimizer failure");
        })
        .join()
        .is_err()
    );
    let q = core.mix_preparation.queue_view();
    assert_eq!(q.effective, OrderMode::Order);
    assert_eq!(*q.rows, entries(&tracks));
    assert!(q.error.is_some());
    let retry = Arc::new(OrderingJob::new(tracks.clone()));
    *core.mix_preparation.arranging.lock().unwrap() = Some(retry.clone());
    set_order_mode(&core, OrderMode::Best, tracks.clone());
    assert!(core.mix_preparation.queue_view().error.is_none());
    let best = vec![tracks[0], tracks[2], tracks[1]];
    assert!(publish_order(
        &core,
        &retry,
        best.clone(),
        Some(Stamp::current(&core)),
        false
    ));
    assert_eq!(*core.mix_preparation.queue_view().rows, entries(&tracks));
    core.mix_preparation
        .publish_queue(core.mix_preparation.queue_intent(), entries(&best));
    assert_eq!(core.mix_preparation.queue_view().effective, OrderMode::Best);
}
