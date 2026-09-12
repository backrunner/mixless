//! Screen-aligned coverage sampling removes moving subpixel seams between bars.
use super::*;

fn strip(
    window: &mut Window,
    vertical: bool,
    axis: f32,
    width: f32,
    top: f32,
    bottom: f32,
    color: Rgba,
    scale: f32,
) {
    let top = top * scale;
    let bottom = bottom * scale;
    if bottom <= top {
        return;
    }
    let mut fill = |start: f32, end: f32, alpha: f32| {
        if end <= start || alpha <= 0. {
            return;
        }
        let mut color = color;
        color.a *= alpha;
        if vertical {
            quad_fill(
                window,
                start / scale,
                axis,
                (end - start) / scale,
                width,
                color,
            );
        } else {
            quad_fill(
                window,
                axis,
                start / scale,
                width,
                (end - start) / scale,
                color,
            );
        }
    };
    if top.floor() == bottom.floor() {
        fill(top.floor(), top.floor() + 1., bottom - top);
        return;
    }
    fill(top.floor(), top.ceil(), top.ceil() - top);
    fill(top.ceil(), bottom.floor(), 1.);
    fill(bottom.floor(), bottom.ceil(), bottom - bottom.floor());
}

pub(super) fn paint(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    vertical: bool,
    data: &WaveCache,
    frame: f64,
    frames: f64,
    fpp: f64,
    anchor: f64,
) {
    paint_sampled(
        window,
        bounds,
        vertical,
        data.columns as f64,
        frame,
        frames,
        fpp,
        anchor,
        |x, cpp| data.sample(x, cpp),
    );
}

pub(super) fn paint_sampled(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    vertical: bool,
    columns: f64,
    frame: f64,
    frames: f64,
    fpp: f64,
    anchor: f64,
    sample: impl Fn(f64, f32) -> cache::Column,
) {
    let scale = window.scale_factor();
    let (origin, cross, span, thick) = if vertical {
        (
            pf(bounds.origin.y),
            pf(bounds.origin.x),
            pf(bounds.size.height),
            pf(bounds.size.width),
        )
    } else {
        (
            pf(bounds.origin.x),
            pf(bounds.origin.y),
            pf(bounds.size.width),
            pf(bounds.size.height),
        )
    };
    let center = cross + thick * 0.5;
    let half = (thick * 0.5 - 4.).max(1.);
    let step = (scale * 0.75).ceil().max(1.) as usize;
    let first = (origin * scale).floor() as i32;
    let end = ((origin + span) * scale).ceil() as i32;
    let cpp = (fpp * columns / frames * (step as f64 / scale as f64)) as f32;
    for body in [false, true] {
        window.paint_layer(bounds, |window| {
            for pixel in (first..end).step_by(step) {
                let start = (pixel as f32 / scale).max(origin);
                let end = ((pixel + step as i32) as f32 / scale).min(origin + span);
                let source = frame + (((start + end) * 0.5 - origin) as f64 - anchor) * fpp;
                if source < 0. || source >= frames {
                    continue;
                }
                let column = sample(source / frames * columns, cpp);
                let (pos, neg, color) = if body {
                    let color = Rgba {
                        r: column.color.r * 0.58,
                        g: column.color.g * 0.58,
                        b: column.color.b * 0.58,
                        a: 1.,
                    };
                    (
                        column.rms.min(column.pos) * 0.82,
                        column.rms.min(column.neg) * 0.82,
                        color,
                    )
                } else {
                    (column.pos, column.neg, column.color)
                };
                strip(
                    window,
                    vertical,
                    start,
                    end - start,
                    center - pos * half,
                    center + neg * half,
                    color,
                    scale,
                );
            }
        });
    }
}
