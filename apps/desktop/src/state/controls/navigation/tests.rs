use super::*;
use gpui::{TestAppContext, point, px, size};

#[gpui::test]
fn deck_navigation_uses_native_hit_targets_and_keeps_playback_state(cx: &mut TestAppContext) {
    let (_temp, core, tracks) = crate::automix::tests::fixture();
    crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
    crate::analysis::load(&core, DeckId::B, tracks[1], || true).unwrap();
    let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core.clone(), cx));
    cx.simulate_resize(size(px(1440.), px(900.)));
    cx.run_until_parked();

    // The displayed time itself is the hit target; changes are per deck and saved.
    let time = cx.debug_bounds("deck-time-A").unwrap().center();
    cx.simulate_click(time, Default::default());
    cx.run_until_parked();
    view.read_with(cx, |s, _| assert_eq!(s.deck_elapsed, [true, false]));
    assert_eq!(
        crate::settings::SettingsStore::load(core.settings.path.clone())
            .get()
            .deck_elapsed,
        [true, false]
    );
    let time = cx.debug_bounds("strip-time-A").unwrap().center();
    cx.simulate_click(time, Default::default());
    cx.run_until_parked();
    view.read_with(cx, |s, _| assert_eq!(s.deck_elapsed, [false, false]));

    // Ring mouse-down auditions near its absolute target. Release commits it.
    let g = PlatterGeometry::new(cx.debug_bounds("jog-A").unwrap());
    let right = point(px(g.cx + g.radius * 0.95), px(g.cy));
    let bottom = point(px(g.cx), px(g.cy + g.radius * 0.95));
    cx.simulate_mouse_down(right, gpui::MouseButton::Left, Default::default());
    view.read_with(cx, |s, _| {
        assert!(matches!(s.drag, Some(DragCtl::Progress { .. })))
    });
    core.engine.render_offline(256);
    let d = core.engine.snapshot().decks[0].clone();
    assert!(d.frame.abs_diff(d.frames / 4) < 1920);
    assert!(!d.playing);
    cx.simulate_mouse_move(bottom, Some(gpui::MouseButton::Left), Default::default());
    cx.simulate_mouse_up(bottom, gpui::MouseButton::Left, Default::default());
    core.engine.render_offline(256);
    let d = core.engine.snapshot().decks[0].clone();
    assert!(d.frame.abs_diff(d.frames / 2) <= 1);
    view.read_with(cx, |s, _| assert!(s.drag.is_none()));

    // A vertical mouse wheel also works on horizontal lanes; trackpad X works too.
    for (layout, delta) in [
        (WaveLayout::Top, point(px(0.), px(-24.))),
        (WaveLayout::Top, point(px(-24.), px(0.))),
        (WaveLayout::Center, point(px(0.), px(-24.))),
    ] {
        core.settings.update(|s| s.wave_layout = layout).unwrap();
        view.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        let position = cx.debug_bounds("wave-lane-A").unwrap().center();
        let before = core.engine.snapshot().decks[0].frame;
        for delta in [delta, point(-delta.x, -delta.y)] {
            cx.simulate_mouse_move(position, None, Default::default());
            cx.simulate_event(gpui::ScrollWheelEvent {
                position,
                delta: gpui::ScrollDelta::Pixels(delta),
                modifiers: Default::default(),
                touch_phase: gpui::TouchPhase::Moved,
            });
            cx.run_until_parked();
            core.engine.render_offline(256);
            if f32::from(delta.x) < 0. || f32::from(delta.y) < 0. {
                assert!(
                    core.engine.snapshot().decks[0].frame > before,
                    "layout={layout:?} delta={delta:?} before={before} after={} position={position:?}",
                    core.engine.snapshot().decks[0].frame
                );
            }
        }
        view.update(cx, |s, _| s.end_wheel_scrub());
        core.engine.render_offline(256);
        assert!(core.engine.snapshot().decks[0].frame.abs_diff(before) <= 1);
        assert!(!core.engine.snapshot().decks[0].playing);
    }
}

#[gpui::test]
fn navigation_gestures_are_audible_only_on_master_while_playing(cx: &mut TestAppContext) {
    let (_temp, core, tracks) = crate::automix::tests::fixture();
    crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.)));
    for playing in [false, true] {
        if playing {
            core.engine
                .dispatch(Command::PlayPause { deck: DeckId::A })
                .unwrap();
        }
        for gesture in 0..5 {
            core.engine
                .dispatch(Command::Seek {
                    deck: DeckId::A,
                    frame: 48_000,
                })
                .unwrap();
            core.engine.render_offline(512);
            view.update(cx, |s, _| {
                s.snapshot = core.engine.snapshot();
                match gesture {
                    0 => {
                        s.begin_wave(DeckId::A, false, 100., 100.);
                        s.drag_move(76., 100.);
                    }
                    1 => {
                        s.begin_wave(DeckId::A, true, 100., 100.);
                        s.drag_move(100., 76.);
                    }
                    2 => {
                        s.begin_jog(DeckId::A, 100., 100., bounds);
                        s.drag_move(100., 76.);
                    }
                    3 => s.begin_progress(DeckId::A, 190., 100., PlatterGeometry::new(bounds)),
                    _ => {
                        s.scroll_wave(DeckId::A, -12., 100.);
                        s.scroll_wave(DeckId::A, -12., 100.);
                    }
                }
            });
            let audio = core.engine.render_offline(1024);
            let audible = audio.iter().any(|v| v.abs() > 0.001);
            assert_eq!(audible, playing, "gesture {gesture}, playing {playing}");
            assert_eq!(core.engine.snapshot().decks[0].playing, playing);
            view.update(cx, |s, _| {
                if gesture == 4 {
                    assert!(s.wheel_scrub.is_some());
                    assert!(s.needs_continuous_repaint());
                    s.wheel_scrub.as_mut().unwrap().1 -= std::time::Duration::from_millis(121);
                    assert!(s.poll_wheel_scrub());
                    assert!(s.wheel_scrub.is_none());
                } else {
                    s.end_drag();
                }
            });
            let released = core.engine.render_offline(2048);
            if !playing {
                assert!(released.iter().all(|v| v.abs() < 1e-8));
            }
            assert_eq!(core.engine.snapshot().decks[0].playing, playing);
        }
    }
}

#[gpui::test]
fn wheel_release_is_bounded_and_new_drag_owns_its_deck(cx: &mut TestAppContext) {
    let (_temp, core, tracks) = crate::automix::tests::fixture();
    for (deck, track) in [DeckId::A, DeckId::B].into_iter().zip(tracks) {
        crate::analysis::load(&core, deck, track, || true).unwrap();
    }
    let view = cx.new(|cx| UiState::new(core.clone(), cx));
    view.update(cx, |s, _| {
        s.scroll_wave(DeckId::A, -10., 100.);
        assert!(!s.poll_wheel_scrub());
        s.scroll_wave(DeckId::B, -20., 100.);
        assert_eq!(s.wheel_scrub.unwrap().0, DeckId::B);
        // Deactivation and a new pointer grab both use end_drag.
        s.end_drag();
        assert!(s.wheel_scrub.is_none());
    });
    core.engine.render_offline(512);
    assert_eq!(core.engine.snapshot().decks[0].frame, 1000);
    assert_eq!(core.engine.snapshot().decks[1].frame, 2000);
    assert!(core.engine.render_offline(512).iter().all(|v| *v == 0.));
}

#[gpui::test]
fn jog_sensitivity_scales_angular_and_label_gestures(cx: &mut TestAppContext) {
    let (_temp, core, tracks) = crate::automix::tests::fixture();
    crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
    let state = cx.new(|cx| UiState::new(core.clone(), cx));
    let bounds = Bounds::new(point(px(0.), px(0.)), size(px(200.), px(200.)));
    for (start, end) in [((170., 100.), (100., 170.)), ((100., 100.), (100., 80.))] {
        let mut movement = Vec::new();
        for sensitivity in [0.5, 2.] {
            core.settings
                .update(|s| s.jog_sensitivity = sensitivity)
                .unwrap();
            core.engine
                .dispatch(Command::Seek {
                    deck: DeckId::A,
                    frame: 48_000,
                })
                .unwrap();
            core.engine.render_offline(256);
            state.update(cx, |s, _| {
                s.snapshot = core.engine.snapshot();
                s.begin_jog(DeckId::A, start.0, start.1, bounds);
                s.drag_move(end.0, end.1);
                s.end_drag();
            });
            core.engine.render_offline(256);
            movement.push(core.engine.snapshot().decks[0].frame - 48_000);
        }
        assert!(movement[0] > 0);
        assert!(movement[1].abs_diff(movement[0] * 4) <= 4);
    }
}
