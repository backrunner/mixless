//! Scrolling, beat-anchored RGB waveform in the Serato/rekordbox style:
//! playhead fixed at the center, with asymmetric peaks, three frequency bands,
//! an RMS body and transient detail. Dragging the lane scrubs the deck through
//! the same Jog command the platter uses.

use std::sync::{Arc, Mutex};

use gpui::prelude::*;
use gpui::{
    Bounds, IntoElement, MouseButton, MouseDownEvent, PathBuilder, Pixels, Rgba, SharedString,
    Window, canvas, div, point, px,
};
use mixless_protocol::{DeckId, DeckSnapshot, Waveform};

use crate::controls::{pf, quad_fill};
use crate::state::UiState;
use crate::theme;

/// Visible window width in beats (4 bars).
const BEATS_SHOWN: f32 = 16.0;
/// Fallback window when BPM is unknown: 8 seconds.
const FALLBACK_SECONDS: f32 = 8.0;

/// The band and RMS boundaries of one waveform slice, ordered from the outer
/// negative side through the positive side.
struct WaveSample {
    position: f32,
    /// high | mid | low | low | mid | high, followed by RMS -/+
    edges: [f32; 8],
    crest: f32,
}

fn wave_point(
    vertical: bool,
    ox: f32,
    oy: f32,
    axis: f32,
    sample: &WaveSample,
    edge: usize,
) -> gpui::Point<Pixels> {
    if vertical {
        point(px(ox + axis + sample.edges[edge]), px(oy + sample.position))
    } else {
        point(px(ox + sample.position), px(oy + axis + sample.edges[edge]))
    }
}

/// Add a filled strip between two waveform boundaries. A whole frequency
/// band is submitted as one GPU path instead of one quad per pixel.
fn append_strip(
    path: &mut PathBuilder,
    samples: &[WaveSample],
    vertical: bool,
    ox: f32,
    oy: f32,
    axis: f32,
    first_edge: usize,
    second_edge: usize,
) {
    let Some(first) = samples.first() else { return };
    path.move_to(wave_point(vertical, ox, oy, axis, first, first_edge));
    for sample in samples.iter().skip(1) {
        path.line_to(wave_point(vertical, ox, oy, axis, sample, first_edge));
    }
    for sample in samples.iter().rev() {
        path.line_to(wave_point(vertical, ox, oy, axis, sample, second_edge));
    }
    path.close();
}

/// Add one continuous boundary without creating a draw call per sample.
fn append_contour(
    path: &mut PathBuilder,
    samples: &[WaveSample],
    vertical: bool,
    ox: f32,
    oy: f32,
    axis: f32,
    edge: usize,
) {
    let Some(first) = samples.first() else { return };
    path.move_to(wave_point(vertical, ox, oy, axis, first, edge));
    for sample in samples.iter().skip(1) {
        path.line_to(wave_point(vertical, ox, oy, axis, sample, edge));
    }
}

fn transient_point(
    vertical: bool,
    ox: f32,
    oy: f32,
    axis: f32,
    sample: &WaveSample,
    offset: f32,
) -> gpui::Point<Pixels> {
    if vertical {
        point(px(ox + axis + offset), px(oy + sample.position))
    } else {
        point(px(ox + sample.position), px(oy + axis + offset))
    }
}

/// Percussive peaks get short cyan-white caps. They are all tessellated into
/// one path, so their detail is effectively free from a submission standpoint.
fn append_transient_caps(
    path: &mut PathBuilder,
    samples: &[WaveSample],
    vertical: bool,
    ox: f32,
    oy: f32,
    axis: f32,
) {
    for sample in samples.iter().filter(|sample| sample.crest > 0.38) {
        for edge in [sample.edges[0], sample.edges[5]] {
            if edge.abs() < 2.0 {
                continue;
            }
            path.move_to(transient_point(vertical, ox, oy, axis, sample, edge * 0.82));
            path.line_to(transient_point(vertical, ox, oy, axis, sample, edge));
        }
    }
}

fn paint_band(window: &mut Window, path: PathBuilder, color: Rgba) {
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

/// Frames per pixel along the time axis for the current view. Shared by the
/// paint code and the drag-to-seek handler so scrubbing tracks the pixels 1:1.
pub fn frames_per_px(d: &DeckSnapshot, device_sr: u32, span: f32) -> f32 {
    let span = span.max(1.0);
    let sr = if d.src_sample_rate > 0 {
        d.src_sample_rate
    } else {
        device_sr
    };
    if sr == 0 {
        return 1.0;
    }
    let beat_frames = if d.sounding_bpm > 0.0 {
        sr as f32 * 60.0 / d.sounding_bpm
    } else {
        0.0
    };
    let window_frames = if beat_frames > 0.0 {
        BEATS_SHOWN * beat_frames
    } else {
        FALLBACK_SECONDS * sr as f32
    };
    window_frames / span
}

pub fn paint_wave(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    vertical: bool,
    d: &DeckSnapshot,
    wave: Option<&Waveform>,
    device_sr: u32,
    deck_color: Rgba,
) {
    let (span, thick) = if vertical {
        (pf(bounds.size.height), pf(bounds.size.width))
    } else {
        (pf(bounds.size.width), pf(bounds.size.height))
    };
    let axis = thick / 2.0;
    let max_half = axis - 2.0;
    let anchor = span / 2.0;
    let ox = pf(bounds.origin.x);
    let oy = pf(bounds.origin.y);

    let Some(data) = wave else { return };
    if data.columns == 0 || d.frames == 0 {
        return;
    }
    let sr = if d.src_sample_rate > 0 {
        d.src_sample_rate
    } else {
        device_sr
    };
    if sr == 0 {
        return;
    }
    let bpm = d.sounding_bpm;
    let beat_frames = if bpm > 0.0 {
        sr as f32 * 60.0 / bpm
    } else {
        0.0
    };
    let fpp = frames_per_px(d, device_sr, span);
    // Never trust a partially-written or older cache to have equally-sized
    // vectors. The positive/negative/RMS vectors are optional for backwards
    // compatibility; the four original vectors are the safe common extent.
    let n = (data.columns as usize)
        .min(data.peak.len())
        .min(data.low.len())
        .min(data.mid.len())
        .min(data.high.len());
    if n == 0 {
        return;
    }
    let asymmetric = data.peak_pos.len() >= n && data.peak_neg.len() >= n;
    let has_rms = data.rms.len() >= n;
    let frames = d.frames as f32;
    let frame = d.frame as f32;

    // Saturated RGB band colors: lows red-orange, mids amber, highs cyan.
    let col_low = gpui::rgb(rgba_bytes(
        theme::WF_LOW[0],
        theme::WF_LOW[1],
        theme::WF_LOW[2],
    ));
    let col_mid = gpui::rgb(rgba_bytes(
        theme::WF_MID[0],
        theme::WF_MID[1],
        theme::WF_MID[2],
    ));
    let col_high = gpui::rgb(rgba_bytes(
        theme::WF_HIGH[0],
        theme::WF_HIGH[1],
        theme::WF_HIGH[2],
    ));
    let col_core = gpui::rgb(rgba_bytes(
        theme::WF_CORE[0],
        theme::WF_CORE[1],
        theme::WF_CORE[2],
    ));

    // A hairline keeps silent passages and phase asymmetry readable.
    let center_line = theme::with_alpha(gpui::rgb(0xbfd3dc), 0.10);
    if vertical {
        quad_fill(window, ox + axis - 0.5, oy, 1.0, span, center_line);
    } else {
        quad_fill(window, ox, oy + axis - 0.5, span, 1.0, center_line);
    }

    // Exactly one aggregate sample per screen pixel. Higher-resolution cache
    // columns improve peak accuracy but do not multiply per-frame path points.
    let sample_count = span.ceil().max(1.0) as usize;
    let mut samples = Vec::with_capacity(sample_count);
    for px_i in 0..sample_count {
        let pxi = px_i as f32 + 0.5;
        let pos = frame + (pxi - anchor) * fpp;
        if pos < 0.0 || pos >= frames {
            continue;
        }
        // Sample the whole pixel footprint, centered on its visual position.
        // Peak-of-max preserves transients while weighted band averaging keeps
        // the color stable when several analysis columns land in one pixel.
        let pos0 = (pos - fpp * 0.5).max(0.0);
        let pos1 = (pos + fpp * 0.5).min(frames);
        let c0 = (((pos0 / frames) * n as f32).floor() as usize).min(n - 1);
        let c1 = (((pos1 / frames) * n as f32).ceil() as usize).min(n);
        let c1 = c1.clamp(c0 + 1, n);
        let mut peak = 0u8;
        let (mut peak_pos, mut peak_neg, mut rms) = (0u8, 0u8, 0u8);
        let (mut l, mut m, mut h) = (0u64, 0u64, 0u64);
        for c in c0..c1 {
            peak = peak.max(data.peak[c]);
            if asymmetric {
                peak_pos = peak_pos.max(data.peak_pos[c]);
                peak_neg = peak_neg.max(data.peak_neg[c]);
            }
            if has_rms {
                rms = rms.max(data.rms[c]);
            }
            let weight = if has_rms {
                data.rms[c].max(data.peak[c] / 4).max(1)
            } else {
                data.peak[c].max(1)
            } as u64;
            l += data.low[c] as u64 * weight;
            m += data.mid[c] as u64 * weight;
            h += data.high[c] as u64 * weight;
        }
        let s = (l + m + h).max(1) as f32;
        let sl = (l as f32 / s).clamp(0.0, 1.0);
        let sm = (m as f32 / s).clamp(0.0, 1.0);

        if !asymmetric {
            peak_pos = peak;
            peak_neg = peak;
        }
        if !has_rms {
            rms = ((peak as f32) * 0.58).round() as u8;
        }
        let scale_peak = |value: u8| {
            if value == 0 {
                0.0
            } else {
                ((value as f32 / 255.0).powf(0.68) * max_half).max(0.65)
            }
        };
        let pos_half = scale_peak(peak_pos);
        let neg_half = scale_peak(peak_neg);
        let neg_low = sl * neg_half;
        let neg_mid = neg_low + sm * neg_half;
        let pos_low = sl * pos_half;
        let pos_mid = pos_low + sm * pos_half;
        let rms_half = scale_peak(rms);
        let rms_neg = rms_half.min(neg_half);
        let rms_pos = rms_half.min(pos_half);
        let crest = if peak > 0 {
            1.0 - rms as f32 / peak as f32
        } else {
            0.0
        };
        samples.push(WaveSample {
            position: pxi,
            edges: [
                -neg_half, -neg_mid, -neg_low, pos_low, pos_mid, pos_half, -rms_neg, rms_pos,
            ],
            crest,
        });
    }

    // Five visual slices become three draw submissions (one per color),
    // versus up to span * 5 individual quads in the old painter.
    let mut high = PathBuilder::fill();
    append_strip(&mut high, &samples, vertical, ox, oy, axis, 0, 1);
    append_strip(&mut high, &samples, vertical, ox, oy, axis, 4, 5);
    paint_band(window, high, col_high);

    let mut mid = PathBuilder::fill();
    append_strip(&mut mid, &samples, vertical, ox, oy, axis, 1, 2);
    append_strip(&mut mid, &samples, vertical, ox, oy, axis, 3, 4);
    paint_band(window, mid, col_mid);

    let mut low = PathBuilder::fill();
    append_strip(&mut low, &samples, vertical, ox, oy, axis, 2, 3);
    paint_band(window, low, col_low);

    // The RMS body adds density to sustained material without hiding the RGB
    // split, while a fine contour preserves crisp peaks on Retina displays.
    let mut core = PathBuilder::fill();
    append_strip(&mut core, &samples, vertical, ox, oy, axis, 6, 7);
    if let Ok(path) = core.build() {
        window.paint_path(path, theme::with_alpha(col_core, 0.18));
    }

    let mut contour = PathBuilder::stroke(px(0.8));
    append_contour(&mut contour, &samples, vertical, ox, oy, axis, 0);
    append_contour(&mut contour, &samples, vertical, ox, oy, axis, 5);
    if let Ok(path) = contour.build() {
        window.paint_path(path, theme::with_alpha(col_core, 0.38));
    }

    let mut transients = PathBuilder::stroke(px(1.0));
    append_transient_caps(&mut transients, &samples, vertical, ox, oy, axis);
    if let Ok(path) = transients.build() {
        window.paint_path(path, theme::with_alpha(col_core, 0.72));
    }

    // Beat ticks + bar lines (grid assumes beat 1 at frame 0).
    if beat_frames > 0.0 {
        let start_beat = ((frame - anchor * fpp) / beat_frames).floor().max(0.0) as i64;
        let end_beat = ((frame + anchor * fpp) / beat_frames).ceil() as i64 + 1;
        for k in start_beat..end_beat {
            let p = anchor + (k as f32 * beat_frames - frame) / fpp;
            if p < 0.0 || p >= span {
                continue;
            }
            let bar = k % 4 == 0;
            let (alpha, w) = if bar { (0.28, 1.0) } else { (0.09, 0.6) };
            let color = theme::with_alpha(gpui::rgb(0xffffff), alpha);
            if vertical {
                quad_fill(window, ox, p + oy, thick, w, color);
            } else {
                quad_fill(window, p + ox, oy, w, thick, color);
            }
        }
    }

    // Hot cues that fall inside the window.
    for (i, cue) in d.cues.iter().enumerate() {
        let Some(cue_frame) = cue else { continue };
        let p = anchor + (*cue_frame as f32 - frame) / fpp;
        if p < -8.0 || p > span + 8.0 {
            continue;
        }
        let color = theme::cue_color(i);
        if vertical {
            quad_fill(window, ox, p + oy, thick, 1.0, color);
            let mut b = PathBuilder::fill();
            b.move_to(gpui::point(px(ox), px(p + oy)));
            b.line_to(gpui::point(px(ox + 8.0), px(p + oy - 5.0)));
            b.line_to(gpui::point(px(ox + 8.0), px(p + oy + 5.0)));
            b.close();
            if let Ok(path) = b.build() {
                window.paint_path(path, color);
            }
        } else {
            quad_fill(window, p + ox, oy, 1.0, thick, color);
            let mut b = PathBuilder::fill();
            b.move_to(gpui::point(px(p + ox), px(oy)));
            b.line_to(gpui::point(px(p + ox + 6.0), px(oy + 8.0)));
            b.line_to(gpui::point(px(p + ox - 6.0), px(oy + 8.0)));
            b.close();
            if let Ok(path) = b.build() {
                window.paint_path(path, color);
            }
        }
    }

    // Dim the played side (behind the playhead).
    let dim = theme::with_alpha(gpui::rgb(0x040508), 0.46);
    if vertical {
        quad_fill(window, ox, oy, thick, anchor, dim);
    } else {
        quad_fill(window, ox, oy, anchor, thick, dim);
    }

    // Playhead: deck-colored glow + white core.
    if vertical {
        quad_fill(
            window,
            axis - 4.0 + ox,
            anchor - 1.0 + oy,
            8.0,
            2.0,
            theme::with_alpha(deck_color, 0.30),
        );
        quad_fill(
            window,
            ox,
            anchor - 1.0 + oy,
            thick,
            2.0,
            gpui::rgb(0xf4f6fa),
        );
    } else {
        quad_fill(
            window,
            anchor - 1.0 + ox,
            axis - 4.0 + oy,
            2.0,
            8.0,
            theme::with_alpha(deck_color, 0.30),
        );
        quad_fill(
            window,
            anchor - 1.0 + ox,
            oy,
            2.0,
            thick,
            gpui::rgb(0xf4f6fa),
        );
    }
}

fn rgba_bytes(r: f32, g: f32, b: f32) -> u32 {
    let to8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    (to8(r) << 16) | (to8(g) << 8) | to8(b)
}

/* ---- drag-to-seek -------------------------------------------------------- */

/// Shared drag cell: bounds + current frames-per-px, refreshed on every paint.
pub type WaveDragCell = Arc<Mutex<(Bounds<Pixels>, f32)>>;

pub fn wave_drag_cell() -> WaveDragCell {
    Arc::new(Mutex::new((Bounds::<Pixels>::default(), 1.0)))
}

/// Mouse-down handler wiring for a waveform lane.
pub fn wave_drag_handler(
    cell: WaveDragCell,
    deck: DeckId,
    vertical: bool,
    cx: &mut gpui::Context<UiState>,
) -> impl Fn(&MouseDownEvent, &mut Window, &mut gpui::App) + 'static {
    cx.listener(move |s: &mut UiState, ev: &MouseDownEvent, window, cx| {
        window.prevent_default();
        let (_, fpp) = *cell.lock().unwrap();
        let pos = if vertical {
            f32::from(ev.position.y)
        } else {
            f32::from(ev.position.x)
        };
        s.begin_wave(deck, vertical, pos, fpp);
        cx.notify();
    })
}

/// Update the drag cell during paint (call inside the canvas paint closure).
pub fn store_drag_info(
    cell: &WaveDragCell,
    bounds: Bounds<Pixels>,
    d: &DeckSnapshot,
    device_sr: u32,
    vertical: bool,
) {
    let span = if vertical {
        pf(bounds.size.height)
    } else {
        pf(bounds.size.width)
    };
    *cell.lock().unwrap() = (bounds, frames_per_px(d, device_sr, span));
}

/* ---- lanes ---------------------------------------------------------------- */

/// Bare waveform lane (no info header) with drag-to-seek. Used by the vertical
/// center layout; also the inner lane of the top strips.
pub fn wave_lane(
    id: SharedString,
    vertical: bool,
    deck: DeckId,
    d: DeckSnapshot,
    wave: Option<Arc<Waveform>>,
    device_sr: u32,
    deck_color: Rgba,
    cx: &mut gpui::Context<UiState>,
) -> impl IntoElement {
    let cell = wave_drag_cell();
    let cell_paint = cell.clone();
    let down = wave_drag_handler(cell, deck, vertical, cx);

    div()
        .id(id)
        .flex_1()
        .min_h_0()
        .min_w_0()
        .w_full()
        .rounded(px(4.))
        .bg(theme::PANEL_INSET)
        .border_1()
        .border_color(theme::LINE_SOFT)
        .overflow_hidden()
        .on_mouse_down(MouseButton::Left, down)
        .child(
            canvas(
                move |_, _, _| {},
                move |bounds, _, window, _| {
                    store_drag_info(&cell_paint, bounds, &d, device_sr, vertical);
                    let wave = wave.clone();
                    paint_wave(
                        window,
                        bounds,
                        vertical,
                        &d,
                        wave.as_deref(),
                        device_sr,
                        deck_color,
                    );
                },
            )
            .size_full(),
        )
}
