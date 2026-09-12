//! Numbered bottom flags share cue-pad colors at every waveform scale.
use super::*;
const DIGITS: [[u8; 7]; 9] = [
    [4, 12, 4, 4, 4, 4, 14],
    [14, 17, 1, 2, 4, 8, 31],
    [30, 1, 1, 14, 1, 1, 30],
    [2, 6, 10, 18, 31, 2, 2],
    [31, 16, 16, 30, 1, 1, 30],
    [14, 16, 16, 30, 17, 17, 14],
    [31, 1, 2, 4, 8, 8, 8],
    [14, 17, 17, 14, 17, 17, 14],
    [31, 4, 4, 4, 4, 4, 4], // deck-local temporary cue: T
];

pub(super) fn flag(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    vertical: bool,
    p: f32,
    index: usize,
    tier: usize,
) {
    if index >= 9 {
        return;
    }
    let color = if index == 8 {
        theme::DANGER
    } else {
        theme::cue_color(index)
    };
    let x = pf(bounds.origin.x);
    let y = pf(bounds.origin.y);
    let w = pf(bounds.size.width);
    let h = pf(bounds.size.height);
    let (bx, by) = if vertical {
        quad_fill(window, x, y + p, w, 1., color);
        (x + w - 14., (y + p + 1.).clamp(y, y + h - 12.))
    } else {
        let by = y + h - 12. - tier as f32 * 12.;
        quad_fill(window, x + p, y + 2., 1., (by - y + 10.).max(1.), color);
        ((x + p + 1.).clamp(x, x + w - 13.), by)
    };
    quad_fill(window, bx, by, 12., 11., color);
    for (row, bits) in DIGITS[index].iter().enumerate() {
        for col in 0..5 {
            if bits & (1 << (4 - col)) != 0 {
                quad_fill(
                    window,
                    bx + 3. + col as f32,
                    by + 2. + row as f32,
                    1.,
                    1.,
                    gpui::rgb(0x08090c),
                );
            }
        }
    }
}

pub(super) fn flags(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    vertical: bool,
    positions: impl IntoIterator<Item = (f32, usize)>,
) {
    let span = pf(if vertical {
        bounds.size.height
    } else {
        bounds.size.width
    });
    let mut ends = [-20f32; 2];
    let mut ordered = [(f32::INFINITY, 0); 9];
    let mut len = 0;
    for value in positions.into_iter().take(9) {
        ordered[len] = value;
        len += 1;
    }
    ordered[..len].sort_by(|a, b| a.0.total_cmp(&b.0));
    window.paint_layer(bounds, |window| {
        for &(p, index) in &ordered[..len] {
            if !(0.0..=span).contains(&p) {
                continue;
            }
            let tier = if p < ends[0] { 1 } else { 0 };
            ends[tier] = p + 14.;
            flag(window, bounds, vertical, p, index, tier);
        }
    });
}
