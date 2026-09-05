//! Hardware-style canvas controls: knobs, faders, LED meters, jog wheel.

use std::sync::{Arc, Mutex};

use gpui::prelude::*;
use gpui::{
    Bounds, ClickEvent, Context, ElementId, IntoElement, MouseButton, MouseDownEvent, PathBuilder,
    Pixels, Rgba, SharedString, Window, canvas, div, point, px, quad,
};
use mixless_protocol::DeckId;

use crate::state::{FaderCtl, KnobCtl, UiState};
use crate::theme;

/// Pixels → f32 (the field is private in gpui 0.2).
pub fn pf(p: Pixels) -> f32 {
    f32::from(p)
}

/* ---- shared paint helpers -------------------------------------------- */

pub fn quad_fill(
    window: &mut Window,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: impl Into<gpui::Background>,
) {
    window.paint_quad(fill_bounds(x, y, w, h, color));
}

pub fn quad_round(
    window: &mut Window,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    color: impl Into<gpui::Background>,
) {
    window.paint_quad(fill_bounds(x, y, w, h, color).corner_radii(px(r)));
}

fn disc(window: &mut Window, cx: f32, cy: f32, radius: f32, color: impl Into<gpui::Background>) {
    quad_round(
        window,
        cx - radius,
        cy - radius,
        radius * 2.0,
        radius * 2.0,
        radius,
        color,
    );
}

fn quad_ring(window: &mut Window, cx: f32, cy: f32, radius: f32, width: f32, color: Rgba) {
    window.paint_quad(quad(
        Bounds {
            origin: point(px(cx - radius), px(cy - radius)),
            size: gpui::size(px(radius * 2.0), px(radius * 2.0)),
        },
        px(radius),
        gpui::rgba(0x00000000),
        px(width),
        color,
        Default::default(),
    ));
}

fn quad_border(
    window: &mut Window,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    background: impl Into<gpui::Background>,
    border: Rgba,
) {
    window.paint_quad(quad(
        Bounds {
            origin: point(px(x), px(y)),
            size: gpui::size(px(w), px(h)),
        },
        px(r),
        background,
        px(1.),
        border,
        Default::default(),
    ));
}

fn fill_bounds(
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: impl Into<gpui::Background>,
) -> gpui::PaintQuad {
    gpui::fill(
        Bounds {
            origin: point(px(x), px(y)),
            size: gpui::size(px(w), px(h)),
        },
        color,
    )
}

/// Arc path from `start_deg` to `end_deg` around (cx, cy). 0° = up, clockwise.
fn arc(cx: f32, cy: f32, r: f32, start_deg: f32, end_deg: f32, width: f32) -> PathBuilder {
    let mut b = PathBuilder::stroke(px(width));
    // Keep sub-pixel curvature without feeding Lyon dozens of unnecessary
    // segments for every tiny mixer knob on every animation frame. Larger
    // circles receive more segments automatically.
    let max_step_deg = (2.0 * (1.0 - (0.25 / r.max(0.25)).min(1.0)).acos())
        .to_degrees()
        .clamp(6.0, 18.0);
    let steps = (((end_deg - start_deg).abs() / max_step_deg).ceil() as usize).max(1);
    for i in 0..=steps {
        let a = (start_deg + (end_deg - start_deg) * (i as f32 / steps as f32)).to_radians();
        let (x, y) = (cx + r * a.sin(), cy - r * a.cos());
        if i == 0 {
            b.move_to(point(px(x), px(y)));
        } else {
            b.line_to(point(px(x), px(y)));
        }
    }
    b
}

/* ---- knob -------------------------------------------------------------- */

pub struct KnobSpec {
    pub ctl: KnobCtl,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub diameter: f32,
    pub color: Rgba,
    pub label: &'static str,
    pub bipolar: bool,
}

pub fn knob(spec: KnobSpec, cx: &mut Context<UiState>) -> impl IntoElement {
    let KnobSpec {
        ctl,
        value,
        min,
        max,
        diameter,
        color,
        label,
        bipolar,
    } = spec;
    let t = if max > min {
        (value - min) / (max - min)
    } else {
        0.0
    };

    let down = cx.listener(move |s: &mut UiState, ev: &MouseDownEvent, window, cx| {
        window.prevent_default();
        if ev.click_count >= 2 {
            s.reset_knob(ctl);
        } else {
            s.begin_knob(ctl, ev.position.y.into());
        }
        cx.notify();
    });

    div()
        .id(ElementId::Name(SharedString::from(format!("knob-{ctl:?}"))))
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .on_mouse_down(MouseButton::Left, down)
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_knob(window, bounds, t, bipolar, color);
                },
            )
            .flex_none()
            // +6px so the outer value arc isn't clipped by the canvas bounds.
            .size(px(diameter + 6.0)),
        )
        .when(!label.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(8.))
                    .text_color(theme::MUTED)
                    .child(label),
            )
        })
}

fn paint_knob(window: &mut Window, bounds: Bounds<Pixels>, t: f32, bipolar: bool, color: Rgba) {
    // Flat modern style: crisp filled discs and clean arcs read far better at
    // small sizes than layered faux-3D highlights, which blur on 1x displays.
    let cx = pf(bounds.origin.x) + pf(bounds.size.width) / 2.0;
    let cy = pf(bounds.origin.y) + pf(bounds.size.height) / 2.0;
    let r = pf(bounds.size.width) / 2.0;

    let start = -135.0f32;
    let t = t.clamp(0.0, 1.0);
    let end = start + 270.0 * t;

    // Flat nested discs retain the hardware silhouette while avoiding a
    // separately tessellated half-disc highlight for every knob.
    let body_r = r - 2.5;
    disc(window, cx, cy, body_r, gpui::rgb(0x303036));
    disc(window, cx, cy, body_r - 1.0, gpui::rgb(0x3a3a41));

    // Recessed center: dark disc with a thin edge.
    let cap_r = body_r * 0.62;
    disc(window, cx, cy, cap_r + 0.5, gpui::rgb(0x1a1a1f));
    disc(window, cx, cy, cap_r, gpui::rgb(0x232329));

    // Value arc drawn OUTSIDE the body, in the reserved ring gap: thin dark
    // track + solid colored segment. Clean 2px lines stay crisp.
    // The canvas includes 3 px of padding on each side. Keep the stroke
    // inside that padding instead of treating it as part of the knob radius.
    let ring_r = r - 1.5;
    if let Ok(p) = arc(cx, cy, ring_r, start, start + 270.0, 2.0).build() {
        window.paint_path(p, gpui::rgb(0x232327));
    }
    if t > 0.001 {
        let (a, b) = if bipolar {
            if end >= 0.0 { (0.0, end) } else { (end, 0.0) }
        } else {
            (start, end)
        };
        if b > a {
            if let Ok(p) = arc(cx, cy, ring_r, a, b, 2.0).build() {
                window.paint_path(p, color);
            }
        }
    }

    // Pointer: bold white line from the recessed disc to the body rim.
    let angle = end.to_radians();
    let (x0, y0) = (
        cx + (cap_r * 0.2) * angle.sin(),
        cy - (cap_r * 0.2) * angle.cos(),
    );
    let (x1, y1) = (
        cx + (body_r - 2.0) * angle.sin(),
        cy - (body_r - 2.0) * angle.cos(),
    );
    let mut b = PathBuilder::stroke(px(2.5));
    b.move_to(point(px(x0), px(y0)));
    b.line_to(point(px(x1), px(y1)));
    if let Ok(p) = b.build() {
        window.paint_path(p, theme::POINTER);
    }
}

/* ---- fader ------------------------------------------------------------- */

pub struct FaderSpec {
    pub ctl: FaderCtl,
    pub value: f32,
    pub min: f32,
    pub max: f32,
    pub width: f32,
    pub color: Rgba,
    pub ticks: bool,
}

/// Vertical fader: click anywhere in the lane to jump, then drag. The
/// wrapper's height is left to the caller (`.h_full()` / `.flex_1()`).
pub fn fader_v(spec: FaderSpec, cx: &mut Context<UiState>) -> gpui::Stateful<gpui::Div> {
    let FaderSpec {
        ctl,
        value,
        min,
        max,
        width,
        color,
        ticks,
    } = spec;
    let t = if max > min {
        (value - min) / (max - min)
    } else {
        0.0
    };

    let cell = Arc::new(Mutex::new(Bounds::<Pixels>::default()));
    let cell_paint = cell.clone();
    let cell_down = cell;

    let down = cx.listener(move |s: &mut UiState, ev: &MouseDownEvent, window, cx| {
        window.prevent_default();
        if ev.click_count >= 2 {
            s.reset_fader(ctl);
        } else {
            let b = *cell_down.lock().unwrap();
            s.begin_fader(ctl, ev.position.y.into(), b);
        }
        cx.notify();
    });

    div()
        .id(ElementId::Name(SharedString::from(format!(
            "fader-{ctl:?}"
        ))))
        .flex()
        .flex_none()
        .w(px(width + 22.0))
        .items_center()
        .justify_center()
        .on_mouse_down(MouseButton::Left, down)
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    *cell_paint.lock().unwrap() = bounds;
                    paint_fader_v(window, bounds, t, width, color, ticks);
                },
            )
            .h_full()
            .flex_1(),
        )
}

fn paint_fader_v(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    t: f32,
    width: f32,
    color: Rgba,
    ticks: bool,
) {
    let cx = pf(bounds.origin.x) + pf(bounds.size.width) / 2.0;
    let top = pf(bounds.origin.y);
    let h = pf(bounds.size.height);
    let t = t.clamp(0.0, 1.0);

    /* recessed slot: outer frame, dark inner well, top inner shadow and a
     * faint bottom lip so the channel reads as milled into the panel */
    let sx = cx - width / 2.0;
    quad_border(
        window,
        sx - 1.0,
        top,
        width + 2.0,
        h,
        width / 2.0 + 1.0,
        gpui::rgb(0x101013),
        theme::LINE_SOFT,
    );
    quad_round(window, sx, top, width, h, width / 2.0, gpui::rgb(0x050507));
    quad_round(
        window,
        sx,
        top,
        width,
        3.0,
        width / 2.0,
        theme::with_alpha(gpui::black(), 0.55),
    );
    quad_fill(
        window,
        sx,
        top + h - 1.5,
        width,
        1.0,
        theme::with_alpha(gpui::white(), 0.045),
    );

    /* concise scale ticks along both sides; center detent is brighter */
    if ticks {
        let n_ticks = 7;
        for i in 0..n_ticks {
            let ty = top + h * (i as f32 / (n_ticks - 1) as f32);
            let center = i == n_ticks / 2;
            let (tw, th, col) = if center {
                (4.5, 1.2, theme::MUTED)
            } else {
                (3.0, 1.0, theme::TRACK_DARK)
            };
            quad_fill(window, sx - 3.0 - tw, ty - th / 2.0, tw, th, col);
            quad_fill(window, sx + width + 3.0, ty - th / 2.0, tw, th, col);
        }
    }

    /* cap: one shadow, crisp bevels and a recessed deck-colored stripe */
    let cap_h = 26.0f32;
    let cap_w = width + 11.0;
    let cap_y = top + (1.0 - t) * (h - cap_h);
    let cap_x = cx - cap_w / 2.0;
    let r = 3.5f32;

    quad_round(
        window,
        cap_x + 1.0,
        cap_y + 1.5,
        cap_w,
        cap_h,
        r,
        theme::with_alpha(gpui::black(), 0.34),
    );

    // Body base with dark outline.
    quad_border(
        window,
        cap_x,
        cap_y,
        cap_w,
        cap_h,
        r,
        gpui::rgb(0x232329),
        gpui::rgb(0x08080a),
    );
    // One highlight and one shadow are enough at this physical size.
    quad_round(
        window,
        cap_x + 2.0,
        cap_y + 1.0,
        cap_w - 4.0,
        2.0,
        1.0,
        theme::with_alpha(gpui::white(), 0.16),
    );
    quad_round(
        window,
        cap_x + 2.0,
        cap_y + cap_h - 2.5,
        cap_w - 4.0,
        1.5,
        1.0,
        theme::with_alpha(gpui::black(), 0.45),
    );

    // Four grooves remain readable without the former 12-layer ridge stack.
    let stripe_c = cap_y + cap_h / 2.0;
    for offset in [-8.0, -5.0, 5.0, 8.0] {
        quad_fill(
            window,
            cap_x + 3.0,
            stripe_c + offset,
            cap_w - 6.0,
            1.0,
            theme::with_alpha(gpui::black(), 0.36),
        );
    }

    // Center stripe: recessed dark slot and deck-color core.
    quad_round(
        window,
        cap_x + 2.0,
        stripe_c - 3.0,
        cap_w - 4.0,
        6.0,
        1.5,
        gpui::rgb(0x08080a),
    );
    quad_fill(window, cap_x + 2.5, stripe_c - 0.5, cap_w - 5.0, 1.0, color);
}

/* ---- crossfader ---------------------------------------------------------- */

pub fn xfader(value: f32, cx: &mut Context<UiState>) -> impl IntoElement {
    let cell = Arc::new(Mutex::new(Bounds::<Pixels>::default()));
    let cell_paint = cell.clone();
    let cell_down = cell;

    let down = cx.listener(move |s: &mut UiState, ev: &MouseDownEvent, window, cx| {
        window.prevent_default();
        if ev.click_count >= 2 {
            s.reset_xfader();
        } else {
            let b = *cell_down.lock().unwrap();
            s.begin_xfader(ev.position.x.into(), b);
        }
        cx.notify();
    });

    div()
        .id("xfader")
        .flex()
        .flex_1()
        .min_w(px(184.0))
        .h(px(30.0))
        .on_mouse_down(MouseButton::Left, down)
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    *cell_paint.lock().unwrap() = bounds;
                    paint_xfader(window, bounds, value);
                },
            )
            .size_full(),
        )
}

fn paint_xfader(window: &mut Window, bounds: Bounds<Pixels>, value: f32) {
    let left = pf(bounds.origin.x);
    let top = pf(bounds.origin.y);
    let w = pf(bounds.size.width);
    let h = pf(bounds.size.height);
    let cy = top + h / 2.0;

    /* recessed track: frame, dark well, left inner shadow */
    let track_h = 8.0f32;
    let ty = cy - track_h / 2.0;
    quad_border(
        window,
        left,
        ty - 1.0,
        w,
        track_h + 2.0,
        4.5,
        gpui::rgb(0x101013),
        theme::LINE_SOFT,
    );
    quad_round(window, left, ty, w, track_h, 4.0, gpui::rgb(0x050507));
    quad_round(
        window,
        left,
        ty,
        w,
        2.5,
        2.0,
        theme::with_alpha(gpui::black(), 0.55),
    );

    /* fine ticks below the track; center detent brighter and longer */
    let n_ticks = 7;
    for i in 0..n_ticks {
        let tx = left + w * (i as f32 / (n_ticks - 1) as f32);
        let center = i == n_ticks / 2;
        let (tw, th, col) = if center {
            (1.2, 5.0, theme::MUTED)
        } else {
            (1.0, 3.0, theme::TRACK_DARK)
        };
        quad_fill(window, tx - tw / 2.0, ty + track_h + 3.0, tw, th, col);
    }

    /* cap: machined metal with vertical ridge, same shading language as the
     * channel fader caps */
    let t = ((value + 1.0) / 2.0).clamp(0.0, 1.0);
    let cap_w = 18.0f32;
    let cap_h = 22.0f32;
    let cap_x = left + t * (w - cap_w);
    let cap_y = cy - cap_h / 2.0;
    let r = 3.5f32;

    quad_round(
        window,
        cap_x + 1.0,
        cap_y + 1.2,
        cap_w,
        cap_h,
        r,
        theme::with_alpha(gpui::black(), 0.34),
    );

    // Body.
    quad_border(
        window,
        cap_x,
        cap_y,
        cap_w,
        cap_h,
        r,
        gpui::rgb(0x232329),
        gpui::rgb(0x08080a),
    );
    quad_round(
        window,
        cap_x + 2.0,
        cap_y + 1.0,
        cap_w - 4.0,
        2.0,
        1.0,
        theme::with_alpha(gpui::white(), 0.16),
    );
    quad_round(
        window,
        cap_x + 2.0,
        cap_y + cap_h - 2.5,
        cap_w - 4.0,
        1.5,
        1.0,
        theme::with_alpha(gpui::black(), 0.45),
    );

    // Vertical grip grooves.
    for k in 0..3 {
        let gx = cap_x + cap_w / 2.0 - 5.5 + k as f32 * 3.0;
        quad_fill(
            window,
            gx,
            cap_y + 3.0,
            1.0,
            cap_h - 6.0,
            theme::with_alpha(gpui::black(), 0.34),
        );
    }

    // Center ridge line (white, like a crossfader crown).
    quad_fill(
        window,
        cap_x + cap_w / 2.0 - 0.7,
        cap_y + 2.0,
        1.4,
        cap_h - 4.0,
        theme::with_alpha(theme::POINTER, 0.85),
    );
}

/* ---- level meter -------------------------------------------------------- */

/// Dual vertical LED meters. `levels` are linear peaks (0..1+), mapped onto a
/// -50..0 dB scale. Fills the height of its parent.
pub fn meter(levels: [f32; 2], width: f32) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .gap_1()
        .h_full()
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_meter(window, bounds, levels[0], width);
                },
            )
            .flex_1(),
        )
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    paint_meter(window, bounds, levels[1], width);
                },
            )
            .flex_1(),
        )
}

fn meter_fill(v: f32) -> f32 {
    if v <= 0.0003 {
        0.0
    } else {
        let db = 20.0 * v.log10();
        ((db + 50.0) / 50.0).clamp(0.0, 1.0)
    }
}

fn paint_meter(window: &mut Window, bounds: Bounds<Pixels>, level: f32, width: f32) {
    let x = pf(bounds.origin.x) + (pf(bounds.size.width) - width) / 2.0;
    let top = pf(bounds.origin.y);
    let h = pf(bounds.size.height);
    quad_round(window, x, top, width, h, 2.0, theme::PANEL_INSET);

    let fill = meter_fill(level);
    if fill <= 0.0 {
        return;
    }

    // Three continuous ranges replace one quad per 3 px LED cell. At mixer
    // height that removes hundreds of draw submissions per active frame while
    // keeping the familiar green / amber / red level thresholds.
    let inner_x = x + 0.5;
    let inner_w = width - 1.0;
    let green = fill.min(0.75);
    if green > 0.0 {
        quad_fill(
            window,
            inner_x,
            top + h * (1.0 - green),
            inner_w,
            h * green,
            theme::LED_GREEN,
        );
    }
    let amber = (fill.min(0.92) - 0.75).max(0.0);
    if amber > 0.0 {
        quad_fill(
            window,
            inner_x,
            top + h * (1.0 - 0.75 - amber),
            inner_w,
            h * amber,
            theme::WARN,
        );
    }
    let red = (fill - 0.92).max(0.0);
    if red > 0.0 {
        quad_fill(
            window,
            inner_x,
            top + h * (1.0 - 0.92 - red),
            inner_w,
            h * red,
            theme::LED_RED,
        );
    }
}

/* ---- jog wheel ----------------------------------------------------------- */

pub struct JogSpec {
    pub deck: DeckId,
    pub frame: u64,
    pub frames: u64,
    pub src_sample_rate: u32,
    pub playing: bool,
    pub color: Rgba,
    pub initial: char,
}

/// Jog wheel that fills its container. Scrubbing is angular: circular pointer
/// gestures rotate the platter (one revolution = 1.8 s of audio, matching the
/// 33⅓ rpm marker); vertical drags near the center still work linearly.
pub fn jog(spec: JogSpec, cx: &mut Context<UiState>) -> impl IntoElement {
    let JogSpec {
        deck,
        frame,
        frames,
        src_sample_rate,
        playing,
        color,
        initial,
    } = spec;

    let cell = Arc::new(Mutex::new(Bounds::<Pixels>::default()));
    let cell_paint = cell.clone();
    let cell_down = cell;

    let down = cx.listener(move |s: &mut UiState, ev: &MouseDownEvent, window, cx| {
        window.prevent_default();
        let b = *cell_down.lock().unwrap();
        s.begin_jog(deck, ev.position.x.into(), ev.position.y.into(), b);
        cx.notify();
    });
    let dbl = cx.listener(move |s: &mut UiState, ev: &ClickEvent, _window, cx| {
        if let ClickEvent::Mouse(mouse) = ev
            && mouse.up.click_count >= 2
        {
            s.play_pause(deck);
            cx.notify();
        }
    });

    div()
        .id(ElementId::Name(SharedString::from(format!(
            "jog-{:?}",
            deck
        ))))
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
                    paint_jog(
                        window,
                        bounds,
                        frame,
                        frames,
                        src_sample_rate,
                        playing,
                        color,
                    );
                },
            )
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(26.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme::TEXT)
                .child(initial.to_string()),
        )
}

fn paint_jog(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    frame: u64,
    frames: u64,
    src_sr: u32,
    playing: bool,
    color: Rgba,
) {
    let cx = pf(bounds.origin.x) + pf(bounds.size.width) / 2.0;
    let cy = pf(bounds.origin.y) + pf(bounds.size.height) / 2.0;
    let r = (pf(bounds.size.width).min(pf(bounds.size.height)) / 2.0 - 6.0).max(8.0);

    // Static circular surfaces use GPUI's native rounded quads. Besides being
    // true fills, these avoid repeatedly tessellating thousands of path
    // segments on every animation frame.
    disc(window, cx, cy, r, gpui::rgb(0x202024));
    quad_ring(window, cx, cy, r - 1.0, 1.5, gpui::rgb(0x3d3d44));

    // Platter: dark vinyl with concentric grooves.
    let platter_r = r - 5.0;
    disc(window, cx, cy, platter_r, gpui::rgb(0x0e0e11));
    // Grooves: alternating light/dark rings, denser toward the outside.
    let mut gr = platter_r - 6.0;
    let mut i = 0;
    while gr > platter_r * 0.42 {
        let alpha = if i % 2 == 0 { 0.16 } else { 0.07 };
        quad_ring(
            window,
            cx,
            cy,
            gr,
            1.0,
            theme::with_alpha(gpui::white(), alpha).into(),
        );
        gr -= 3.5;
        i += 1;
    }

    // Track progress ring just inside the bezel (Serato/rekordbox style).
    let pos = if frames > 0 {
        (frame as f32 / frames as f32).clamp(0.0, 1.0)
    } else {
        0.0
    };
    quad_ring(window, cx, cy, r - 3.0, 2.0, gpui::rgb(0x1a1a1e));
    if pos > 0.001 {
        if let Ok(p) = arc(cx, cy, r - 3.0, -90.0, -90.0 + 360.0 * pos, 2.5).build() {
            window.paint_path(p, theme::with_alpha(color, 0.9));
        }
    }

    // Spinning marker at ~33 1/3 rpm: glow underlay + bright radial needle.
    let spin = if src_sr > 0 {
        (((frame as f64 / src_sr as f64) * 200.0) % 360.0) as f32
    } else {
        0.0
    };
    let a = spin.to_radians();
    let (x0, y0) = (
        cx + (platter_r * 0.48) * a.sin(),
        cy - (platter_r * 0.48) * a.cos(),
    );
    let (x1, y1) = (
        cx + (platter_r - 7.0) * a.sin(),
        cy - (platter_r - 7.0) * a.cos(),
    );
    let mut marker = |width: f32, col: gpui::Hsla| {
        let mut b = PathBuilder::stroke(px(width));
        b.move_to(point(px(x0), px(y0)));
        b.line_to(point(px(x1), px(y1)));
        if let Ok(p) = b.build() {
            window.paint_path(p, col);
        }
    };
    marker(7.0, theme::with_alpha(color, 0.25));
    marker(3.0, color.into());

    // Play halo around the platter edge.
    if playing {
        quad_ring(
            window,
            cx,
            cy,
            platter_r + 1.5,
            2.0,
            theme::with_alpha(color, 0.45).into(),
        );
    }

    // Center label: recessed well with a deck-colored ring.
    let lr = r * 0.24;
    disc(window, cx, cy, lr, gpui::rgb(0x101013));
    disc(window, cx, cy, lr - 2.5, theme::PANEL_RAISED);
    quad_ring(
        window,
        cx,
        cy,
        lr - 1.0,
        1.5,
        theme::with_alpha(color, 0.6).into(),
    );
}

/// Tiny caption under a control group.
pub fn caption(text: &str) -> gpui::Div {
    div()
        .text_size(px(7.))
        .text_color(theme::MUTED)
        .child(text.to_string())
}
