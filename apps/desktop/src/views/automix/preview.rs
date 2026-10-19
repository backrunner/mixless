//! Native rendering of production toolbar states, without starting playback.
//! MIXLESS_DATA_DIR must be an isolated temporary directory, as in settings previews.
use super::*;
use gpui::{Application, Bounds, WindowBounds, WindowOptions, size};
use mixless_protocol::{MixPlanSummary, Polyline, TrackId};

struct Preview(Vec<(&'static str, gpui::Entity<UiState>)>);

impl gpui::Render for Preview {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let width = f32::from(window.viewport_size().width);
        div()
            .size_full()
            .bg(theme::BG)
            .flex()
            .flex_col()
            .children(self.0.iter().map(|(name, state)| {
                div()
                    .flex()
                    .flex_col()
                    .pb(px(12.))
                    .child(
                        div()
                            .px(px(12.))
                            .py(px(8.))
                            .text_size(px(11.))
                            .text_color(theme::MUTED)
                            .child(*name),
                    )
                    .child(state.update(cx, |s, cx| s.render_topbar(width, cx)))
            }))
    }
}

pub(crate) fn run() -> bool {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--preview-automix") {
        return false;
    }
    let data = std::path::PathBuf::from(
        std::env::var_os("MIXLESS_DATA_DIR").expect("isolated preview data"),
    );
    assert!(data.is_absolute() && data.starts_with(std::env::temp_dir()));
    assert_eq!(std::env::var("MIXLESS_OFFLINE").as_deref(), Ok("1"));
    assert_eq!(std::env::var("MIXLESS_MODELS_OFFLINE").as_deref(), Ok("1"));
    let width = args
        .get(2)
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(1440.)
        .max(1180.);
    Application::new()
        .with_assets(crate::icons::Assets)
        .run(move |cx| {
            let core = crate::state::app_core();
            let shutdown = core.clone();
            cx.on_app_quit(move |_| {
                shutdown
                    .shutting_down
                    .store(true, std::sync::atomic::Ordering::Release);
                async {}
            })
            .detach();
            cx.on_window_closed(|cx| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(width), px(460.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |_, cx| {
                    cx.new(|cx| {
                        Preview(
                            [
                                "Scheduled",
                                "Incoming stems closed",
                                "Independent stem exchange",
                                "Outgoing channel closed",
                                "Paused",
                            ]
                            .into_iter()
                            .enumerate()
                            .map(|(i, label)| {
                                let state = cx.new(|cx| {
                                    let mut s = UiState::new(core.clone(), cx);
                                    s.automix_active = true;
                                    s.automix_plan = Some((
                                        DeckId::B,
                                        MixPlan {
                                            summary: Some(MixPlanSummary {
                                                pair: (TrackId(1), TrackId(2)),
                                                strategy: S::StemSwap,
                                                score: 1.,
                                                length_bars: 8,
                                                used_fallback: false,
                                            }),
                                            t_in_a: 10.,
                                            t_out_a: 18.,
                                            t_in_b: 0.,
                                            t_end_b: 8.,
                                            clock: Polyline {
                                                nodes: vec![(0., 0.), (8., 8.)],
                                            },
                                            ..Default::default()
                                        }
                                        .into(),
                                    ));
                                    s.snapshot.decks[1].frame =
                                        if i == 0 { 0 } else { 12 * 44_100 };
                                    s.snapshot.decks[1].src_sample_rate = 44_100;
                                    s.snapshot.decks[0].stems_ready = true;
                                    s.snapshot.decks[0].fader = if i <= 1 { 0. } else { 1. };
                                    s.snapshot.decks[0].stem_gain =
                                        if i <= 1 { [0.; 4] } else { [0., 1., 0., 0.] };
                                    s.snapshot.decks[1].fader = if i == 3 { 0. } else { 1. };
                                    s.snapshot.automix_progress = if i == 0 { 0. } else { 0.25 };
                                    s.snapshot.automix_paused = i == 4;
                                    s
                                });
                                (label, state)
                            })
                            .collect(),
                        )
                    })
                },
            )
            .expect("open AutoMix preview");
            cx.activate(true);
        });
    true
}
