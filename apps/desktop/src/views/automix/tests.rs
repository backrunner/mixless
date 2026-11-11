use super::*;
use gpui::{TestAppContext, size};
use mixless_protocol::{MixPlanSummary, MixStage, Polyline, TrackId};

fn plan() -> MixPlan {
    let mut p = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (TrackId(1), TrackId(2)),
            strategy: S::PhraseBlend,
            score: 0.8,
            used_fallback: false,
            length_bars: 8,
        }),
        t_in_a: 10.,
        t_out_a: 18.,
        t_in_b: 2.,
        t_end_b: 10.,
        clock: Polyline {
            nodes: vec![(0., 0.), (8., 8.)],
        },
        stages: vec![
            MixStage {
                start_bar: 0.,
                end_bar: 4.,
                label: "Prepare shared tempo · incoming silent".into(),
            },
            MixStage {
                start_bar: 4.,
                end_bar: 8.,
                label: "Layer rhythm · hold vocal foreground until the outgoing phrase finishes"
                    .into(),
            },
            MixStage {
                start_bar: 8.,
                end_bar: 10.,
                label: "Vocal gap · lift outgoing vocal with echo tail".into(),
            },
        ],
        ..Default::default()
    };
    p.outgoing_source = Polyline {
        nodes: vec![(0., 10.), (8., 18.)],
    };
    p.incoming_source = Polyline {
        nodes: vec![(0., 2.), (8., 10.)],
    };
    p
}

fn live(snapshot: &mut EngineSnapshot, out: DeckId) {
    snapshot.automix_on = true;
    snapshot.decks[out.index()].track_id = Some(TrackId(1));
    snapshot.decks[1 - out.index()].track_id = Some(TrackId(2));
    for deck in &mut snapshot.decks {
        deck.playing = true;
    }
}

#[test]
fn transition_details_preserve_windows_stages_and_playback_status() {
    for out in [DeckId::A, DeckId::B] {
        let mut snapshot = EngineSnapshot::default();
        live(&mut snapshot, out);
        let ready = Transition::new(out, &plan(), &snapshot);
        assert_eq!(ready.name, "Phrase blend");
        assert_eq!(ready.phase, "Starts in 0:10");
        assert_eq!(ready.status, "In 0:10");
        assert_eq!(ready.windows, ["0:10–0:18", "0:02–0:10"]);
        assert_eq!(ready.stages.len(), 3);
        assert!(ready.stages.iter().all(|s| !s.contains('·')));
        assert_eq!(ready.active_stage, None);
        snapshot.decks[out.index()].frame = 15 * 44_100;
        snapshot.decks[out.index()].send = 0.5;
        snapshot.automix_progress = 0.625;
        let active = Transition::new(out, &plan(), &snapshot);
        assert_eq!(active.phase, "Transition 62%");
        assert_eq!(active.status, "Phrase blend");
        assert_eq!(active.active_stage, Some(1));
        assert!(active.echo.is_some());
        snapshot.automix_paused = true;
        assert_eq!(Transition::new(out, &plan(), &snapshot).phase, "Paused");
        assert_eq!(Transition::new(out, &plan(), &snapshot).status, "Paused");
    }
}

#[test]
fn stem_swap_status_follows_waiting_exchange_and_completion_on_either_deck() {
    let mut plan = plan();
    plan.summary.as_mut().unwrap().strategy = S::StemSwap;
    for out in [DeckId::A, DeckId::B] {
        let incoming = 1 - out.index();
        let mut snapshot = EngineSnapshot::default();
        live(&mut snapshot, out);
        snapshot.decks[out.index()].frame = 12 * 44_100;
        snapshot.automix_progress = 0.25;
        snapshot.decks[incoming].stems_ready = true;
        snapshot.decks[incoming].playing = true;
        snapshot.decks[incoming].stem_gain = [0.; 4];
        let waiting = Transition::new(out, &plan, &snapshot);
        assert_eq!(waiting.status, "Waiting");
        assert_eq!(
            waiting.phase,
            format!(
                "Waiting for {} entry",
                label(if out == DeckId::A {
                    DeckId::B
                } else {
                    DeckId::A
                })
            )
        );
        assert_eq!(
            waiting.name, "Stem swap",
            "detail retains the scheduled technique"
        );
        snapshot.decks[incoming].stem_gain[1] = 1.;
        assert_eq!(Transition::new(out, &plan, &snapshot).status, "Stem swap");
        snapshot.decks[out.index()].stems_ready = true;
        snapshot.decks[out.index()].stem_gain = [1., 0., 1., 1.];
        assert_eq!(Transition::new(out, &plan, &snapshot).status, "Stem swap");
        snapshot.decks[incoming].stem_gain = [1.; 4];
        assert_eq!(Transition::new(out, &plan, &snapshot).status, "Stem swap");
        snapshot.decks[incoming].stem_gain = [0., 1., 0., 0.];
        snapshot.decks[incoming].fader = 0.;
        assert_eq!(Transition::new(out, &plan, &snapshot).status, "Waiting");
        snapshot.decks[incoming].fader = 1.;
        snapshot.decks[out.index()].fader = 0.;
        assert_eq!(Transition::new(out, &plan, &snapshot).status, "Finishing");
        snapshot.automix_paused = true;
        assert_eq!(Transition::new(out, &plan, &snapshot).status, "Paused");
    }
}

#[gpui::test]
fn lifecycle_status_keeps_the_entire_master_control_inside_the_toolbar(cx: &mut TestAppContext) {
    struct Header(gpui::Entity<UiState>);
    impl gpui::Render for Header {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            self.0.update(cx, |s, cx| {
                s.render_topbar(window.viewport_size().width.into(), cx)
            })
        }
    }
    let (_temp, core, _) = crate::automix::tests::fixture();
    let state = cx.new(|cx| UiState::new(core, cx));
    let (header, cx) = cx.add_window_view(|_, _| Header(state.clone()));
    for width in [1180., 1280., 1440.] {
        for scene in 0..5 {
            state.update(cx, |s, _| {
                s.automix_active = true;
                s.automix_plan = Some((DeckId::B, plan().into()));
                s.snapshot = EngineSnapshot::default();
                live(&mut s.snapshot, DeckId::B);
                s.snapshot.decks[1].frame = if scene == 0 { 0 } else { 12 * 44_100 };
                s.snapshot.decks[0].fader = if scene == 1 { 0. } else { 1. };
                s.snapshot.decks[1].fader = if scene == 3 { 0. } else { 1. };
                s.snapshot.automix_paused = scene == 4;
            });
            header.update(cx, |_, cx| cx.notify());
            cx.simulate_resize(size(px(width), px(900.)));
            cx.run_until_parked();
            let master = cx.debug_bounds("master-control").unwrap();
            assert!(
                master.right() <= px(width - 8.),
                "width={width}, scene={scene}, master={master:?}"
            );
        }
    }
}

#[gpui::test]
fn tooltip_wraps_stages_inside_its_surface(cx: &mut TestAppContext) {
    let transition = Transition::new(DeckId::B, &plan(), &EngineSnapshot::default());
    let (_, cx) = cx.add_window_view(|_, _| AutomixTip {
        transition,
        gap: 12.,
        stale: false,
        _subscription: None,
    });
    cx.simulate_resize(size(px(1180.), px(760.)));
    cx.run_until_parked();
    let surface = cx.debug_bounds("automix-detail").unwrap();
    assert_eq!(surface.size.width, px(284.));
    assert!(surface.size.height <= px(320.));
    let mut bottom = surface.top();
    for selector in ["automix-stage-0", "automix-stage-1", "automix-stage-2"] {
        let stage = cx.debug_bounds(selector).unwrap();
        assert!(stage.top() >= bottom);
        assert!(stage.right() <= surface.right() - px(12.));
        assert!(stage.bottom() <= surface.bottom() - px(12.));
        bottom = stage.bottom();
    }
    assert!(cx.debug_bounds("automix-stage-1").unwrap().size.height >= px(28.));
}

#[gpui::test]
fn open_tooltip_follows_countdown_and_pause_changes(cx: &mut TestAppContext) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let state = cx.new(|cx| UiState::new(core, cx));
    state.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_plan = Some((DeckId::A, plan().into()));
        live(&mut s.snapshot, DeckId::A);
    });
    let tip = cx.new(|cx| {
        AutomixTip::new(
            state.read(cx).transition().unwrap(),
            12.,
            state.downgrade(),
            cx,
        )
    });
    for (paused, frame, phase) in [
        (false, 5 * 48_000, "Starts in 0:05"),
        (true, 5 * 48_000, "Paused"),
    ] {
        state.update(cx, |s, cx| {
            s.snapshot.decks[0].frame = frame;
            s.snapshot.decks[0].src_sample_rate = 48_000;
            s.snapshot.automix_paused = paused;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(tip.read_with(cx, |t, _| t.transition.phase.clone()), phase);
    }
}

#[gpui::test]
fn transition_stays_next_to_auto_at_supported_window_widths(cx: &mut TestAppContext) {
    struct Header(gpui::Entity<UiState>);
    impl gpui::Render for Header {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            self.0.update(cx, |s, cx| {
                s.render_topbar(window.viewport_size().width.into(), cx)
            })
        }
    }
    let (_temp, core, _) = crate::automix::tests::fixture();
    let view = cx.new(|cx| UiState::new(core, cx));
    let (_, cx) = cx.add_window_view(|_, _| Header(view.clone()));
    view.update(cx, |s, cx| {
        s.automix_active = true;
        s.automix_plan = Some((DeckId::B, plan().into()));
        live(&mut s.snapshot, DeckId::B);
        cx.notify();
    });
    for width in [1180., 1280., 1440., 1800.] {
        cx.simulate_resize(size(px(width), px(900.)));
        cx.run_until_parked();
        let auto = cx.debug_bounds("automix-toggle").unwrap();
        let status = cx.debug_bounds("automix-transition").unwrap();
        let order = cx.debug_bounds("automix-order").unwrap();
        assert!((status.left() - auto.right() - px(6.)).abs() < px(1.));
        assert!((order.left() - status.right() - px(6.)).abs() < px(1.));
        assert!(auto.left() > px(300.), "width={width}, auto={auto:?}");
        assert!(cx.debug_bounds("automix-skip").unwrap().right() < px(width));
    }
}

#[test]
fn closed_crossfader_and_manual_takeover_cannot_claim_a_stem_exchange() {
    for out in [DeckId::A, DeckId::B] {
        let mut s = EngineSnapshot::default();
        live(&mut s, out);
        s.decks[out.index()].frame = 12 * 44_100;
        s.automix_progress = 0.25;
        s.xfader = if out == DeckId::A { -1. } else { 1. };
        assert_eq!(Transition::new(out, &plan(), &s).status, "Waiting");
        s.xf_reverse = true;
        assert_eq!(Transition::new(out, &plan(), &s).status, "Finishing");
        s.automix_modified = true;
        let manual = Transition::new(out, &plan(), &s);
        assert_eq!(manual.status, "Manual");
        assert_eq!(manual.windows, ["—", "—"]);
        assert!(manual.stages.is_empty());
        assert_eq!(manual.active_stage, None);
    }
}

#[gpui::test]
fn stale_track_identity_and_completed_plan_hide_the_transition(cx: &mut TestAppContext) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let state = cx.new(|cx| UiState::new(core, cx));
    state.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_plan = Some((DeckId::A, plan().into()));
        live(&mut s.snapshot, DeckId::A);
    });
    let tip = cx.new(|cx| {
        AutomixTip::new(
            state.read(cx).transition().unwrap(),
            12.,
            state.downgrade(),
            cx,
        )
    });
    state.update(cx, |s, cx| {
        s.snapshot.decks[1].track_id = Some(TrackId(99));
        assert!(s.transition().is_none());
        live(&mut s.snapshot, DeckId::A);
        s.snapshot.automix_on = false;
        assert!(s.transition().is_none());
        assert!(s.transition_window(DeckId::A).is_none());
        cx.notify();
    });
    cx.run_until_parked();
    assert!(tip.read_with(cx, |t, _| t.stale));
}

#[gpui::test]
fn loading_status_fits_its_content_and_keeps_toolbar_controls_visible(cx: &mut TestAppContext) {
    struct Header(gpui::Entity<UiState>);
    impl gpui::Render for Header {
        fn render(
            &mut self,
            window: &mut gpui::Window,
            cx: &mut gpui::Context<Self>,
        ) -> impl IntoElement {
            self.0.update(cx, |s, cx| {
                s.render_topbar(window.viewport_size().width.into(), cx)
            })
        }
    }
    let (_temp, core, _) = crate::automix::tests::fixture();
    let state = cx.new(|cx| UiState::new(core, cx));
    state.update(cx, |s, _| {
        s.automix_active = true;
        s.automix_loading = true;
        s.automix_preparation_progress = Some(0.42);
        s.automix_status = "Finding the best order".into();
    });
    let (header, cx) = cx.add_window_view(|_, _| Header(state.clone()));
    for width in [1180., 1280., 1440., 1800.] {
        cx.simulate_resize(size(px(width), px(900.)));
        cx.run_until_parked();
        let status = cx.debug_bounds("automix-status").unwrap();
        let auto = cx.debug_bounds("automix-toggle").unwrap();
        let order = cx.debug_bounds("automix-order").unwrap();
        assert!(cx.debug_bounds("automix-loading").is_some());
        assert!((status.left() - auto.right() - px(6.)).abs() < px(1.));
        assert!((order.left() - status.right() - px(6.)).abs() < px(1.));
        assert!(auto.left() > px(260.), "width={width}, auto={auto:?}");
        assert!(cx.debug_bounds("master-mixer-open").unwrap().right() < px(width));
    }
    let long = cx.debug_bounds("automix-status").unwrap().size.width;
    state.update(cx, |s, _| {
        s.automix_loading = false;
        s.automix_preparation_progress = None;
        s.automix_status = "Waiting".into();
    });
    header.update(cx, |_, cx| cx.notify());
    cx.run_until_parked();
    // GPUI retains old debug selectors across frames; measure the current row.
    assert!(cx.debug_bounds("automix-status").unwrap().size.width < px(45.));
    assert!(cx.debug_bounds("automix-status").unwrap().size.width < long - px(50.));
}
