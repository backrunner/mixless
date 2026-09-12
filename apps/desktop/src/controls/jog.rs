//! Compact hardware-style platter: fixed artwork, a rotating position marker
//! and a separate track-progress ring. Geometry also defines scratch gestures.
use super::*;
use gpui::{ClickEvent, RenderImage, linear_color_stop, linear_gradient, rgb};
use mixless_protocol::DeckId;

pub struct JogSpec {
    pub deck: DeckId,
    /// The same interpolated presentation clock used by the scrolling waveform.
    pub frame: f64,
    pub frames: u64,
    pub src_sample_rate: u32,
    pub playing: bool,
    pub color: Rgba,
    pub initial: char,
    pub artwork: Option<Arc<RenderImage>>,
    pub end_warning: Option<f32>,
}

/// One revolution is 1.8 s of audio (33⅓ rpm). The artwork has no independent
/// mouse handlers, so scratching and double-click transport work across it.
pub fn jog(spec: JogSpec, cx: &mut Context<UiState>) -> impl IntoElement {
    let deck = spec.deck;
    let initial = spec.initial;
    let has_artwork = spec.artwork.is_some();
    let end_warning = spec.end_warning;
    let cell = Arc::new(Mutex::new(Bounds::<Pixels>::default()));
    let cell_paint = cell.clone();
    let down = cx.listener(move |s: &mut UiState, ev: &MouseDownEvent, window, cx| {
        window.prevent_default();
        cx.stop_propagation();
        let bounds = *cell.lock().unwrap();
        s.begin_jog(deck, ev.position.x.into(), ev.position.y.into(), bounds);
        cx.notify();
    });
    let dbl = cx.listener(move |s: &mut UiState, ev: &ClickEvent, _, cx| {
        if let ClickEvent::Mouse(mouse) = ev
            && mouse.up.click_count >= 2
        {
            s.play_pause(deck);
            cx.notify();
        }
    });

    div()
        .id(ElementId::Name(SharedString::from(format!("jog-{deck:?}"))))
        .flex()
        .flex_1()
        .min_h_0()
        .w_full()
        .relative()
        .on_mouse_down(MouseButton::Left, down)
        .on_click(dbl)
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    *cell_paint.lock().unwrap() = bounds;
                    paint_jog(window, bounds, &spec);
                },
            )
            .size_full(),
        )
        .when(!has_artwork, |el| {
            el.child(
                div()
                    .absolute()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(28.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(theme::with_alpha(theme::TEXT, 0.72))
                    .child(initial.to_string()),
            )
        })
        .when_some(end_warning, |el, seconds| {
            el.child(
                div()
                    .absolute()
                    .bottom(px(5.))
                    .w_full()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(1.))
                            .rounded(px(4.))
                            .bg(rgb(0x201318))
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::LED_RED)
                            .child(format!("−{:.0}s", seconds.ceil())),
                    ),
            )
        })
}

fn paint_jog(window: &mut Window, bounds: Bounds<Pixels>, spec: &JogSpec) {
    let cx = pf(bounds.origin.x) + pf(bounds.size.width) / 2.;
    let cy = pf(bounds.origin.y) + pf(bounds.size.height) / 2.;
    let r = (pf(bounds.size.width).min(pf(bounds.size.height)) / 2. - 6.).max(8.);
    let color = spec.color;

    // Native rounded quads keep the circular surfaces smooth at Retina scale;
    // three broad rings replace the dense, shimmering vinyl-line texture.
    disc(
        window,
        cx,
        cy + 1.,
        r + 2.,
        theme::with_alpha(gpui::black(), 0.30),
    );
    disc(
        window,
        cx,
        cy,
        r,
        linear_gradient(
            155.,
            linear_color_stop(rgb(0x51565e), 0.),
            linear_color_stop(rgb(0x1a1d23), 1.),
        ),
    );
    quad_ring(window, cx, cy, r, 0.75, rgb(0x646973));
    disc(window, cx, cy, r - 2., rgb(0x090c10));
    disc(
        window,
        cx,
        cy,
        r * 0.91,
        linear_gradient(
            145.,
            linear_color_stop(rgb(0x262b32), 0.),
            linear_color_stop(rgb(0x11151a), 1.),
        ),
    );
    quad_ring(window, cx, cy, r * 0.91, 0.75, rgb(0x3d434d));
    quad_ring(window, cx, cy, r * 0.80, 0.75, rgb(0x303640));
    quad_ring(window, cx, cy, r * 0.76, 0.75, rgb(0x0b0e13));
    paint_ticks(window, cx, cy, r);

    // Track progress starts at twelve o'clock and stays separate from rotation.
    let progress = if spec.frames > 0 {
        (spec.frame / spec.frames as f64).clamp(0., 1.) as f32
    } else {
        0.
    };
    quad_ring(
        window,
        cx,
        cy,
        r * 0.95,
        2.2,
        theme::with_alpha(color, 0.12).into(),
    );
    if progress > 0. {
        paint_arc(
            window,
            cx,
            cy,
            r * 0.95,
            0.,
            progress * 360.,
            2.2,
            theme::with_alpha(color, if spec.playing { 1. } else { 0.65 }),
        );
    }
    // A stationary zero marker makes remaining progress easy to read.
    quad_round(
        window,
        cx - 1.,
        cy - r * 0.98,
        2.,
        r * 0.055,
        1.,
        rgb(0xd7dee7),
    );

    let hub = r * 0.63;
    disc(window, cx, cy, hub + 2., rgb(0x070a0e));
    if let Some(image) = &spec.artwork {
        // The worker already cropped and resized this image. Only a cached
        // texture is painted here; no decoding, resizing or image rotation.
        let image_bounds = Bounds {
            origin: point(px(cx - hub), px(cy - hub)),
            size: gpui::size(px(hub * 2.), px(hub * 2.)),
        };
        let _ = window.paint_image(image_bounds, px(hub).into(), image.clone(), 0, false);
    } else {
        disc(
            window,
            cx,
            cy,
            hub,
            linear_gradient(
                145.,
                linear_color_stop(rgb(0x262d37), 0.),
                linear_color_stop(rgb(0x121820), 1.),
            ),
        );
        quad_ring(window, cx, cy, hub * 0.91, 0.75, rgb(0x343c47));
        quad_ring(window, cx, cy, hub * 0.84, 0.5, rgb(0x222a35));
    }
    quad_ring(
        window,
        cx,
        cy,
        hub + 1.,
        1.,
        theme::with_alpha(color, 0.35).into(),
    );

    let spin = if spec.src_sample_rate > 0 {
        (spec.frame / spec.src_sample_rate as f64 * 200.).rem_euclid(360.) as f32
    } else {
        0.
    };
    // Short index arc plus a bright needle remain visible around light artwork.
    paint_arc(
        window,
        cx,
        cy,
        r * 0.73,
        spin - 7.,
        spin + 7.,
        4.,
        theme::with_alpha(color, 0.18),
    );
    paint_arc(
        window,
        cx,
        cy,
        r * 0.73,
        spin - 5.,
        spin + 5.,
        1.5,
        color.into(),
    );
    let a = spin.to_radians();
    let mut marker = PathBuilder::stroke(px(2.5));
    marker.move_to(point(
        px(cx + r * 0.66 * a.sin()),
        px(cy - r * 0.66 * a.cos()),
    ));
    marker.line_to(point(
        px(cx + r * 0.79 * a.sin()),
        px(cy - r * 0.79 * a.cos()),
    ));
    if let Ok(path) = marker.build() {
        window.paint_path(path, rgb(0xf1f5fa));
    }

    if let Some(seconds) = spec.end_warning {
        let speed = if seconds <= 10. { 2. } else { 1. };
        let clock = spec.frame / spec.src_sample_rate.max(1) as f64;
        let pulse = 0.5 + 0.5 * (clock * speed * std::f64::consts::TAU).sin() as f32;
        quad_ring(
            window,
            cx,
            cy,
            r,
            2.5,
            theme::with_alpha(theme::LED_RED, 0.3 + pulse * 0.7).into(),
        );
    }
}

fn paint_ticks(window: &mut Window, cx: f32, cy: f32, r: f32) {
    // Batch all tick strokes in two paths, instead of one draw per tick.
    for major in [false, true] {
        let mut ticks = PathBuilder::stroke(px(if major { 1.2 } else { 0.8 }));
        for i in 0..60 {
            if (i % 5 == 0) != major {
                continue;
            }
            let a = (i as f32 * 6.).to_radians();
            let inner = if major { 0.825 } else { 0.85 };
            ticks.move_to(point(
                px(cx + r * inner * a.sin()),
                px(cy - r * inner * a.cos()),
            ));
            ticks.line_to(point(
                px(cx + r * 0.88 * a.sin()),
                px(cy - r * 0.88 * a.cos()),
            ));
        }
        if let Ok(path) = ticks.build() {
            window.paint_path(path, if major { rgb(0x858e9b) } else { rgb(0x4b5461) });
        }
    }
}

fn paint_arc(
    window: &mut Window,
    cx: f32,
    cy: f32,
    r: f32,
    start: f32,
    end: f32,
    width: f32,
    color: gpui::Hsla,
) {
    // Tighter curvature than the shared knob helper: the larger progress ring
    // must look circular even when a maximized window makes the platter large.
    let steps = ((end - start).abs() / 3.).ceil().max(1.) as usize;
    let mut path = PathBuilder::stroke(px(width));
    for i in 0..=steps {
        let angle = (start + (end - start) * i as f32 / steps as f32).to_radians();
        let p = point(px(cx + r * angle.sin()), px(cy - r * angle.cos()));
        if i == 0 {
            path.move_to(p);
        } else {
            path.line_to(p);
        }
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}
