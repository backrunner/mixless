use super::*;
use crate::theme;
use gpui::{IntoElement, PathBuilder, Rgba, Window, canvas, div, point, prelude::*, px};
use std::sync::{Arc, Mutex};

pub(super) fn db(v: f32) -> f32 {
    20. * v.max(1e-6).log10()
}
pub(super) fn db_text(v: f32) -> String {
    if v < 1e-6 {
        "−∞ dBFS".into()
    } else {
        format!("{:.1} dBFS", db(v))
    }
}
fn line(
    window: &mut Window,
    points: impl IntoIterator<Item = (f32, f32)>,
    color: Rgba,
    width: f32,
) {
    let mut path = PathBuilder::stroke(px(width));
    for (i, (x, y)) in points.into_iter().enumerate() {
        if i == 0 {
            path.move_to(point(px(x), px(y)));
        } else {
            path.line_to(point(px(x), px(y)));
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
fn dot(window: &mut Window, x: f32, y: f32, r: f32, color: Rgba) {
    window.paint_quad(gpui::quad(
        gpui::Bounds::new(
            point(px(x - r), px(y - r)),
            gpui::size(px(2. * r), px(2. * r)),
        ),
        px(r),
        color,
        px(0.),
        color,
        Default::default(),
    ));
}
fn grid(window: &mut Window, b: Bounds<Pixels>, cols: usize, rows: usize) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = (
        f32::from(b.origin.x),
        f32::from(b.origin.y),
        f32::from(b.size.width),
        f32::from(b.size.height),
    );
    for i in 0..=cols {
        let xx = x + w * i as f32 / cols as f32;
        line(window, [(xx, y), (xx, y + h)], theme::LINE, 1.);
    }
    for i in 0..=rows {
        let yy = y + h * i as f32 / rows as f32;
        line(window, [(x, yy), (x + w, yy)], theme::LINE, 1.);
    }
    (x, y, w, h)
}

pub(super) fn eq_graph(state: &UiState, cx: &mut gpui::Context<UiState>) -> gpui::AnyElement {
    let p = state.snapshot.master_chain;
    let band = state.master_mixer.band;
    let sr = state.snapshot.sample_rate as f32;
    let cell = Arc::new(Mutex::new(Bounds::<Pixels>::default()));
    let painted = cell.clone();
    div()
        .id("master-eq-graph")
        .debug_selector(|| "master-eq-graph".into())
        .h(px(146.))
        .w_full()
        .bg(theme::PANEL_INSET)
        .cursor_crosshair()
        .on_mouse_down(
            gpui::MouseButton::Left,
            cx.listener(move |s, e: &gpui::MouseDownEvent, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
                s.begin_master_eq(
                    *cell.lock().unwrap(),
                    e.position.x.into(),
                    e.position.y.into(),
                );
                cx.notify();
            }),
        )
        .child(
            canvas(
                move |_, _, _| {},
                move |b, _, window, _| {
                    *painted.lock().unwrap() = b;
                    let (x, y, w, h) = grid(window, b, 6, 4);
                    let col = if p.eq_enabled {
                        theme::ACCENT
                    } else {
                        theme::MUTED
                    };
                    line(
                        window,
                        (0..=240).map(|i| {
                            let t = i as f32 / 240.;
                            let value = mixless_engine::master_eq_response(
                                &p.eq,
                                (20. * 1000_f32.powf(t)).min(sr * 0.49),
                                sr,
                            );
                            (x + t * w, y + (12. - value.clamp(-12., 12.)) / 24. * h)
                        }),
                        col,
                        2.,
                    );
                    line(
                        window,
                        [(x, y + h / 2.), (x + w, y + h / 2.)],
                        theme::TRACK_DARK,
                        1.,
                    );
                    for (i, v) in p.eq.iter().enumerate() {
                        let xx = x + (v.frequency_hz / 20.).log10() / 3. * w;
                        let yy = y + (12. - v.gain_db) / 24. * h;
                        dot(
                            window,
                            xx,
                            yy,
                            if i == band { 6. } else { 4. },
                            if i == band {
                                theme::ACCENT
                            } else {
                                theme::TEXT
                            },
                        );
                        dot(window, xx, yy, 2., theme::PANEL);
                    }
                },
            )
            .size_full(),
        )
        .into_any_element()
}

pub(super) fn transfer(
    p: MasterChainSettings,
    m: mixless_protocol::MasterStageMeter,
    dynamics: bool,
) -> gpui::AnyElement {
    let enabled = if dynamics {
        p.dynamics.enabled
    } else {
        p.compressor.enabled
    };
    canvas(
        move |_, _, _| {},
        move |b, _, window, _| {
            let (x, y, w, h) = grid(window, b, 4, 4);
            let pos = |input: f32, output: f32| {
                (
                    x + (input + 80.).clamp(0., 80.) / 80. * w,
                    y + (1. - (output + 80.).clamp(0., 80.) / 80.) * h,
                )
            };
            line(
                window,
                [pos(-80., -80.), pos(0., 0.)],
                theme::TRACK_DARK,
                1.,
            );
            line(
                window,
                (0..=240).map(|i| {
                    let input = -80. + i as f32 / 3.;
                    let output = if dynamics {
                        input - p.dynamics.reduction_db(input)
                    } else {
                        input - p.compressor.reduction_db(input) + p.compressor.makeup_db
                    };
                    pos(input, output)
                }),
                if enabled { theme::ACCENT } else { theme::MUTED },
                2.,
            );
            let input = db(m.input[0].max(m.input[1]));
            let output = db(m.output[0].max(m.output[1]));
            if input > -80. {
                let (xx, yy) = pos(input, output);
                dot(window, xx, yy, 4., theme::LED_GREEN);
            }
        },
    )
    .w_full()
    .h(px(120.))
    .into_any_element()
}

pub(super) fn history(history: VecDeque<MasterChainMeters>, ceiling: f32) -> gpui::AnyElement {
    canvas(
        move |_, _, _| {},
        move |b, _, window, _| {
            let (x, y, w, h) = grid(window, b, 5, 4);
            let yy = |v: f32| y + (6. - v.clamp(-60., 6.)) / 66. * h * 0.7;
            line(
                window,
                [(x, yy(ceiling)), (x + w, yy(ceiling))],
                theme::DANGER,
                1.,
            );
            for (which, col) in [(0, theme::MUTED), (1, theme::LED_GREEN), (2, theme::ACCENT)] {
                line(
                    window,
                    history.iter().enumerate().map(|(i, m)| {
                        let xx = x + w * (150 - history.len() + i) as f32 / 149.;
                        let level = match which {
                            0 => yy(db(m.limiter.input[0].max(m.limiter.input[1]))),
                            1 => yy(db(m.limiter.output[0].max(m.limiter.output[1]))),
                            _ => y + h * 0.72 + m.limiter.reduction_db.min(24.) / 24. * h * 0.28,
                        };
                        (xx, level)
                    }),
                    col,
                    1.5,
                );
            }
        },
    )
    .h(px(120.))
    .w_full()
    .into_any_element()
}
