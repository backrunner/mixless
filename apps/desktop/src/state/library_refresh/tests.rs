use super::*;
use gpui::{Entity, TestAppContext};
use std::time::{Duration, Instant};

fn drain(view: &Entity<UiState>, cx: &mut TestAppContext) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let done = view.update(cx, |s, _| {
            s.poll_library_order();
            s.poll_library();
            !s.library_order.is_pending()
                && s.library_refresh.rx.is_none()
                && !s.library_refresh.again
        });
        if done {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "library did not finish refreshing"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[gpui::test]
fn cached_best_to_uncached_source_warms_without_switching_back_to_best(cx: &mut TestAppContext) {
    let (_temp, core, ids) = crate::automix::tests::fixture();
    let next = vec![ids[2], ids[1], ids[0]];
    let source = core
        .library
        .replace_playlist("Cached source", &ids)
        .unwrap()
        .0;
    let target = core
        .library
        .replace_playlist("Uncached source", &next)
        .unwrap()
        .0;
    crate::automix::tests::cache_best_order(&core, &ids, &[2, 0, 1]);
    let key = crate::automix::tests::best_order_key(&core, &next);
    assert!(
        core.library
            .load_best_order(&key, next.len())
            .unwrap()
            .is_none()
    );
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| s.select_playlist(Some(source)));
    drain(&view, cx);
    view.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_playlist = Some(source);
        s.automix_tracks = ids.clone();
        s.cycle_automix_order();
        s.cycle_automix_order();
        assert!(core.mix_preparation.best());
        s.select_playlist(Some(target));
        assert!(!core.mix_preparation.best());
    });
    drain(&view, cx);
    let deadline = Instant::now() + Duration::from_secs(10);
    while core
        .library
        .load_best_order(&key, next.len())
        .unwrap()
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "uncached source did not warm Best"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    view.update(cx, |s, _| s.refresh_tracks());
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert!(
            !core.mix_preparation.best(),
            "cache completion changed the playing order"
        );
        assert_eq!(core.mix_preparation.tracks(), next);
        assert!(!core.mix_preparation.order_chosen());
        s.stop_automix();
    });
}

#[gpui::test]
fn returning_to_best_with_a_changed_playlist_falls_back_and_only_warms_cache(
    cx: &mut TestAppContext,
) {
    let (_temp, core, ids) = crate::automix::tests::fixture();
    let source = core
        .library
        .replace_playlist("Remembered Best", &ids)
        .unwrap()
        .0;
    let other = core
        .library
        .replace_playlist("Other source", &ids[..2])
        .unwrap()
        .0;
    crate::automix::tests::cache_best_order(&core, &ids, &[2, 0, 1]);
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| s.select_playlist(Some(source)));
    drain(&view, cx);
    view.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_playlist = Some(source);
        s.automix_tracks = ids.clone();
        s.cycle_automix_order();
        s.cycle_automix_order();
        assert!(core.mix_preparation.best());
        s.select_playlist(Some(other));
    });
    drain(&view, cx);
    let changed = vec![ids[2], ids[1], ids[0]];
    core.library
        .reorder_tracks(Some(PlaylistId(source)), &ids, &changed)
        .unwrap();
    view.update(cx, |s, _| {
        s.select_playlist(Some(source));
        assert!(
            !core.mix_preparation.best(),
            "remembered BEST must wait for a valid cache"
        );
    });
    drain(&view, cx);
    let key = crate::automix::tests::best_order_key(&core, &changed);
    let deadline = Instant::now() + Duration::from_secs(10);
    while core
        .library
        .load_best_order(&key, changed.len())
        .unwrap()
        .is_none()
    {
        assert!(
            Instant::now() < deadline,
            "fallback did not warm Best in the background"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    // A same-source refresh after completion must still leave this pass in ORDER.
    view.update(cx, |s, _| s.refresh_tracks());
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert!(
            !core.mix_preparation.best(),
            "background completion changed this pass to BEST"
        );
        assert_eq!(core.mix_preparation.tracks(), changed);
        assert!(crate::automix::arranged(&core, &changed).is_none());
        s.stop_automix();
    });
}

#[gpui::test]
fn order_modes_belong_to_the_source_playlist(cx: &mut TestAppContext) {
    let (_temp, core, ids) = crate::automix::tests::fixture();
    crate::automix::tests::cache_best_order(&core, &ids, &[2, 0, 1]);
    let first = core
        .library
        .replace_playlist("Best source", &ids)
        .unwrap()
        .0;
    let second = core
        .library
        .replace_playlist("Order source", &ids[..2])
        .unwrap()
        .0;
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| s.select_playlist(Some(first)));
    drain(&view, cx);
    view.update(cx, |s, _| {
        // Model an active source without starting a competing playback worker.
        s.automix_active = true;
        s.automix_playlist = Some(first);
        s.automix_tracks = ids.clone();
        s.cycle_automix_order(); // SHUFFLE
        s.cycle_automix_order(); // BEST
        assert!(core.mix_preparation.best());
        s.select_playlist(Some(second));
        assert!(
            !core.mix_preparation.best(),
            "new source inherited the previous source's BEST"
        );
        assert!(!s.automix_shuffle.load(Ordering::Acquire));
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert_eq!(s.automix_tracks, ids[..2]);
        assert!(!core.mix_preparation.best());
        s.cycle_automix_order(); // SHUFFLE
        s.cycle_automix_order(); // BEST
        s.cycle_automix_order(); // explicit ORDER
        s.select_playlist(Some(first));
    });
    drain(&view, cx);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !crate::automix::order_ready(&core, &ids) {
        assert!(
            Instant::now() < deadline,
            "returning source cache lookup stalled"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    view.update(cx, |s, _| {
        assert!(
            core.mix_preparation.best(),
            "returning source lost its cached BEST"
        );
        s.select_playlist(Some(second));
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert!(
            !core.mix_preparation.best(),
            "returning source lost its explicit ORDER"
        );
        assert!(!s.automix_shuffle.load(Ordering::Acquire));
        s.stop_automix();
    });
}

#[gpui::test]
fn new_sources_resolve_their_cache_and_all_tracks_has_its_own_choice(cx: &mut TestAppContext) {
    let (_temp, core, ids) = crate::automix::tests::fixture();
    let first = core
        .library
        .replace_playlist("Manual order", &ids[..2])
        .unwrap()
        .0;
    let second = core
        .library
        .replace_playlist("Cached best", &ids)
        .unwrap()
        .0;
    crate::automix::tests::cache_best_order(&core, &ids, &[2, 0, 1]);
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| s.select_playlist(Some(first)));
    drain(&view, cx);
    view.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_playlist = Some(first);
        s.automix_tracks = ids[..2].to_vec();
        for _ in 0..3 {
            s.cycle_automix_order();
        } // explicit ORDER
        assert!(core.mix_preparation.order_chosen());
        s.select_playlist(Some(second));
        assert!(
            !core.mix_preparation.order_chosen(),
            "manual ORDER leaked to a different source"
        );
    });
    drain(&view, cx);
    let deadline = Instant::now() + Duration::from_secs(10);
    while !crate::automix::order_ready(&core, &ids) {
        assert!(Instant::now() < deadline, "new source cache lookup stalled");
        std::thread::sleep(Duration::from_millis(5));
    }
    view.update(cx, |s, _| {
        assert!(core.mix_preparation.best());
        assert_eq!(core.mix_preparation.tracks(), vec![ids[2], ids[0], ids[1]]);
        s.cycle_automix_order(); // explicit ORDER on second
        s.cycle_automix_order(); // explicit SHUFFLE on second
        s.select_playlist(None);
        assert!(
            !s.automix_shuffle.load(Ordering::Acquire),
            "All tracks inherited playlist shuffle"
        );
        assert!(!core.mix_preparation.order_chosen());
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        s.select_playlist(Some(second));
        assert!(
            s.automix_shuffle.load(Ordering::Acquire),
            "playlist shuffle was not restored"
        );
        s.select_playlist(Some(first));
        assert!(!core.mix_preparation.best());
        assert!(!s.automix_shuffle.load(Ordering::Acquire));
        assert!(core.mix_preparation.order_chosen());
        s.stop_automix();
    });
}

#[gpui::test]
fn keep_current_isolates_browsing_reorders_and_refreshes_the_running_source(
    cx: &mut TestAppContext,
) {
    let (_temp, core, ids) = crate::automix::tests::fixture();
    let source = core
        .library
        .replace_playlist("Playing", &ids[..2])
        .unwrap()
        .0;
    let other = core
        .library
        .replace_playlist("Browsing", &ids[1..])
        .unwrap()
        .0;
    core.settings
        .update(|s| s.automix_follow_playlist = false)
        .unwrap();
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| s.select_playlist(Some(source)));
    drain(&view, cx);
    view.update(cx, |s, _| {
        s.toggle_automix();
        s.cycle_automix_order(); // SHUFFLE
        s.cycle_automix_order(); // BEST belongs to the pinned source
        s.select_playlist(Some(other));
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert_eq!(s.automix_playlist, Some(source));
        assert!(
            core.mix_preparation.best(),
            "browsing changed the pinned source's order mode"
        );
        assert_eq!(s.automix_tracks, ids[..2]);
        assert_eq!(s.tracks.iter().map(|t| t.id).collect::<Vec<_>>(), ids[1..]);
        let drag = crate::views::library::TrackDrag {
            playlist: s.playlist_sel,
            tracks: s.tracks.clone(),
            index: 0,
            id: ids[1],
            title: s.tracks[0].title.clone().into(),
        };
        s.reorder_track(&drag, 2);
        assert_eq!(s.automix_tracks, ids[..2]);
    });
    drain(&view, cx);
    assert_eq!(
        core.library
            .playlist_tracks(PlaylistId(other))
            .unwrap()
            .iter()
            .map(|t| t.id)
            .collect::<Vec<_>>(),
        vec![ids[2], ids[1]]
    );
    core.library
        .remove_playlist_track(PlaylistId(source), ids[1])
        .unwrap();
    view.update(cx, |s, _| s.refresh_tracks());
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert_eq!(s.automix_tracks, vec![ids[0]]);
        s.remove_playlist(other);
        assert!(
            s.automix_active,
            "deleting the browsed playlist stopped the source"
        );
        s.remove_playlist(source);
        assert!(
            !s.automix_active,
            "deleting the running source must stop AutoMix"
        );
    });
}

#[gpui::test]
fn follow_selected_ignores_stale_reads_and_restarts_from_the_visible_playlist(
    cx: &mut TestAppContext,
) {
    let (_temp, core, ids) = crate::automix::tests::fixture();
    let first = core.library.replace_playlist("First", &ids[..2]).unwrap().0;
    let second = core
        .library
        .replace_playlist("Second", &ids[1..])
        .unwrap()
        .0;
    let empty = core.library.replace_playlist("Empty", &[]).unwrap().0;
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| s.select_playlist(Some(first)));
    drain(&view, cx);
    view.update(cx, |s, _| {
        s.toggle_automix();
        s.refresh_tracks(); // This read must not replace the later selection.
        s.select_playlist(Some(second));
        assert!(s.automix_tracks.is_empty());
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert_eq!(s.automix_playlist, Some(second));
        assert_eq!(s.automix_tracks, ids[1..]);
        s.select_playlist(Some(empty));
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert!(s.automix_tracks.is_empty());
        s.automix_retry = Some(Instant::now());
        let epoch = s.automix_epoch.load(Ordering::Acquire);
        s.poll_automix();
        assert_eq!(s.automix_epoch.load(Ordering::Acquire), epoch);
        s.select_playlist(None);
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert_eq!(s.automix_playlist, None);
        assert_eq!(s.automix_tracks, ids);
        core.settings
            .update(|s| s.automix_follow_playlist = false)
            .unwrap();
        s.select_playlist(Some(first));
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert_eq!(
            s.automix_playlist, None,
            "All tracks must remain a valid pinned source"
        );
        assert_eq!(s.automix_tracks, ids);
        s.stop_automix();
        s.toggle_automix();
        assert_eq!(s.automix_playlist, Some(first));
        assert_eq!(s.automix_tracks, ids[..2]);
        s.stop_automix();
    });
}

#[gpui::test]
fn retry_uses_the_retained_source_while_an_empty_playlist_is_visible(cx: &mut TestAppContext) {
    let (_temp, core, ids) = crate::automix::tests::fixture();
    let source = core
        .library
        .replace_playlist("Source", &ids[..1])
        .unwrap()
        .0;
    let empty = core.library.replace_playlist("Empty", &[]).unwrap().0;
    core.settings
        .update(|s| s.automix_follow_playlist = false)
        .unwrap();
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| s.select_playlist(Some(source)));
    drain(&view, cx);
    view.update(cx, |s, _| {
        // Model a completed worker awaiting its retry without a competing worker.
        s.automix_active = true;
        s.automix_playlist = Some(source);
        s.prepare_mix_playlist(vec![ids[0]]);
        s.select_playlist(Some(empty));
    });
    drain(&view, cx);
    view.update(cx, |s, _| {
        assert!(s.tracks.is_empty());
        s.automix_retry = Some(Instant::now());
        assert!(s.poll_automix());
        assert!(s.automix_rx.is_some());
        assert!(s.automix_retry.is_none());
    });
    let deadline = Instant::now() + Duration::from_secs(5);
    while core
        .engine
        .snapshot()
        .decks
        .iter()
        .all(|d| d.track_id != Some(ids[0]))
    {
        assert!(
            Instant::now() < deadline,
            "retry did not load the retained source"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    view.update(cx, |s, _| s.stop_automix());
}
