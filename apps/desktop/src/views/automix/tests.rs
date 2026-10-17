use super::*;
use gpui::{TestAppContext, size};
use mixless_protocol::{MixPlanSummary, MixStage, Polyline, TrackId};

fn plan() -> MixPlan {
    MixPlan {
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
    }
}

#[test]
fn transition_details_preserve_windows_stages_and_playback_status() {
    for out in [DeckId::A, DeckId::B] {
        let mut snapshot = EngineSnapshot::default();
        let ready = Transition::new(out, &plan(), &snapshot);
        assert_eq!(ready.name, "Phrase blend");
        assert_eq!(ready.phase, "Starts in 0:10");
        assert_eq!(ready.windows, ["0:10–0:18", "0:02–0:10"]);
        assert_eq!(ready.stages.len(), 3);
        assert!(ready.stages.iter().all(|s| !s.contains('·')));
        assert_eq!(ready.active_stage, None);
        snapshot.decks[out.index()].frame = 15 * 44_100;
        snapshot.decks[out.index()].send = 0.5;
        snapshot.automix_progress = 0.625;
        let active = Transition::new(out, &plan(), &snapshot);
        assert_eq!(active.phase, "Transition 62%");
        assert_eq!(active.active_stage, Some(1));
        assert!(active.echo.is_some());
        snapshot.automix_paused = true;
        assert_eq!(Transition::new(out, &plan(), &snapshot).phase, "Paused");
    }
}

#[gpui::test]
fn tooltip_wraps_stages_inside_its_surface(cx: &mut TestAppContext) {
    let transition = Transition::new(DeckId::B, &plan(), &EngineSnapshot::default());
    let (_, cx) = cx.add_window_view(|_, _| AutomixTip {
        transition,
        gap: 12.,
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
    state.update(cx, |s, _| s.automix_plan = Some((DeckId::A, plan().into())));
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
            s.snapshot.automix_paused = paused;
            cx.notify();
        });
        cx.run_until_parked();
        assert_eq!(tip.read_with(cx, |t, _| t.transition.phase.clone()), phase);
    }
}

#[gpui::test]
fn transition_stays_next_to_auto_at_supported_window_widths(cx: &mut TestAppContext) {
    let (_temp, core, _) = crate::automix::tests::fixture();
    let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core, cx));
    view.update(cx, |s, cx| {
        s.automix_active = true;
        s.automix_plan = Some((DeckId::B, plan().into()));
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
