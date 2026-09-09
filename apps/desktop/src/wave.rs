//! Scrolling, beat-anchored RGB waveform in the Serato/rekordbox style:
//! playhead fixed at the center, with a symmetric peak envelope tinted by
//! frequency energy, a darker RMS body and measured beat ticks. Dragging the lane scrubs the deck through
//! the same Jog command the platter uses.

use std::sync::{Arc, Mutex};

use gpui::prelude::*;
use gpui::{
    Bounds, IntoElement, MouseButton, MouseDownEvent, PathBuilder, Pixels, Rgba, SharedString,
    Window, canvas, div, px,
};
use mixless_protocol::{DeckId, DeckSnapshot, TempoMap, Waveform};

use crate::controls::{pf, quad_fill};
use crate::state::UiState;
use crate::theme;

/// Visible window width in beats (4 bars).
const BEATS_SHOWN: f32 = 16.0;
/// Fallback window when BPM is unknown: 8 seconds.
const FALLBACK_SECONDS: f32 = 8.0;

/// Continuous spectral color: no quantization boundaries to flash during scrolling.
fn spectrum(low: f32, mid: f32, high: f32) -> Rgba {
    let weights = [low.sqrt(), mid.sqrt(), high.sqrt()];
    let total = weights.iter().sum::<f32>().max(1.);
    let rgb: [f32; 3] = std::array::from_fn(|channel| {
        [theme::WF_LOW, theme::WF_MID, theme::WF_HIGH].iter().zip(weights)
            .map(|(color, weight)| color[channel] * weight / total).sum::<f32>()
    });
    let brightest = rgb.iter().copied().fold(0.01, f32::max);
    gpui::rgb(rgba_bytes(rgb[0] / brightest, rgb[1] / brightest, rgb[2] / brightest))
}

fn source_beat_frames(d: &DeckSnapshot, sr: u32) -> f32 {
    if d.sounding_bpm.is_finite() && d.sounding_bpm > 0.0 && d.rate.is_finite() && d.rate > 0.0 {
        sr as f32 * 60.0 * d.rate / d.sounding_bpm
    } else {
        0.0
    }
}

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
    let beat_frames = source_beat_frames(d, sr);
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
    tempo: Option<&TempoMap>,
    device_sr: u32,
    deck_color: Rgba,
) {
    let (span, thick) = if vertical {
        (pf(bounds.size.height), pf(bounds.size.width))
    } else {
        (pf(bounds.size.width), pf(bounds.size.height))
    };
    let axis = thick / 2.0;
    let max_half = (axis - 5.0).max(1.0);

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
    let fpp = frames_per_px(d, device_sr, span);
    let anchor = (d.frame as f32 / fpp).min(span / 2.0);
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
    let has_rms = data.rms.len() >= n;
    let frames = d.frames as f32;
    let frame = d.frame as f32;

    // A hairline keeps silent passages and phase asymmetry readable.
    let center_line = theme::with_alpha(gpui::rgb(0xbfd3dc), 0.10);
    if vertical {
        quad_fill(window, ox + axis - 0.5, oy, 1.0, span, center_line);
    } else {
        quad_fill(window, ox, oy + axis - 0.5, span, 1.0, center_line);
    }

    // Source-anchored bins translate continuously under the playhead. Screen-
    // anchored peak picking changed shape every frame and caused visible shimmer.
    // Rounded quads are GPU instances: no per-frame polygon tessellation.
    let columns_per_pixel = fpp * n as f32 / frames;
    let stride = (columns_per_pixel.max(1.).log2().floor().exp2() as usize).max(1);
    let first = (((frame - anchor * fpp).max(0.) / frames * n as f32) as usize / stride) * stride;
    let last = (((frame + (span - anchor) * fpp).min(frames) / frames * n as f32).ceil() as usize).min(n);
    for c0 in (first..last).step_by(stride) {
        let c1 = (c0 + stride).min(n);
        let p0 = (anchor + (c0 as f32 / n as f32 * frames - frame) / fpp).max(0.);
        let p1 = (anchor + (c1 as f32 / n as f32 * frames - frame) / fpp).min(span);
        if p1 <= p0 { continue; }
        let peak = data.peak[c0..c1].iter().copied().max().unwrap_or(0) as f32 / 255.;
        if peak == 0. { continue; }
        let average = |band: &[u8]| band[c0..c1].iter().map(|v| *v as f32).sum::<f32>() / (c1 - c0) as f32;
        let (l, m, h) = (average(&data.low), average(&data.mid), average(&data.high));
        let half = peak.powf(0.85) * max_half;
        let mut column = |half: f32, color: Rgba| {
            if vertical {
                quad_fill(window, ox + axis - half, oy + p0, half * 2., p1 - p0, color);
            } else {
                quad_fill(window, ox + p0, oy + axis - half, p1 - p0, half * 2., color);
            }
        };
        column(half, spectrum(l, m, h));
        // Bright high-frequency tips, a midrange body, and a warm bass core.
        // Each band uses its measured share; quiet bass cannot fill a cymbal hit.
        let total = (l + m + h).max(1.);
        column(half * ((l + m) / total).sqrt(), spectrum(l, m, 0.));
        let rms = if has_rms { average(&data.rms) / 255. } else { peak * 0.55 };
        column((rms * max_half).min(half) * (l / total).sqrt(), gpui::rgb(0xff6542));
    }

    if d.loop_on {
        let start = (anchor + (d.loop_start_frame as f32 - frame) / fpp).clamp(0., span);
        let end = (anchor + (d.loop_end_frame as f32 - frame) / fpp).clamp(0., span);
        if end > start {
            let color = theme::with_alpha(theme::LED_GREEN, 0.18);
            if vertical { quad_fill(window, ox, oy + start, thick, end - start, color); }
            else { quad_fill(window, ox + start, oy, end - start, thick, color); }
        }
    }

    // Draw measured source-time beats. No invented downbeat at frame zero.
    if let Some(tempo) = tempo {
        let start_sec = (frame - anchor * fpp) / sr as f32;
        let end_sec = (frame + (span - anchor) * fpp) / sr as f32;
        for (positions, downbeat) in [(&tempo.beats, false), (&tempo.downbeats, true)] {
            let start = positions.partition_point(|sec| *sec < start_sec);
            for sec in positions[start..].iter().take_while(|sec| **sec <= end_sec) {
                let p = anchor + (*sec * sr as f32 - frame) / fpp;
                if !(0.0..span).contains(&p) {
                    continue;
                }
                let color =
                    theme::with_alpha(gpui::rgb(0xffffff), if downbeat { 0.65 } else { 0.19 });
                let tick = if downbeat { thick } else { 5.0 };
                if vertical {
                    quad_fill(window, ox, oy + p, tick, 1.0, color);
                    if !downbeat {
                        quad_fill(window, ox + thick - tick, oy + p, tick, 1.0, color);
                    }
                } else {
                    quad_fill(window, ox + p, oy, 1.0, tick, color);
                    if !downbeat {
                        quad_fill(window, ox + p, oy + thick - tick, 1.0, tick, color);
                    }
                }
            }
        }
    }

    if let Some(cue) = d.automix_cue_frame {
        let p = anchor + (cue as f32 - frame) / fpp;
        if (0.0..span).contains(&p) {
            if vertical { quad_fill(window, ox, oy + p, thick, 2., theme::WARN); }
            else { quad_fill(window, ox + p, oy, 2., thick, theme::WARN); }
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
    let dim = theme::with_alpha(gpui::rgb(0x040508), 0.18);
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
        cx.stop_propagation();
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
    tempo: Option<Arc<TempoMap>>,
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
        .cursor(if d.frames == 0 {
            gpui::CursorStyle::Arrow
        } else {
            gpui::CursorStyle::OpenHand
        })
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
                        tempo.as_deref(),
                        device_sr,
                        deck_color,
                    );
                },
            )
            .size_full(),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changing_rate_does_not_move_source_beat_grid() {
        let mut d = DeckSnapshot {
            sounding_bpm: 120.,
            src_sample_rate: 48_000,
            ..Default::default()
        };
        let native = frames_per_px(&d, 44_100, 1000.);
        d.rate = 1.08;
        d.sounding_bpm = 129.6;
        assert!((frames_per_px(&d, 44_100, 1000.) - native).abs() < 0.001);
        assert!((source_beat_frames(&d, 48_000) - 24_000.).abs() < 0.01);
    }
    #[test]
    fn spectral_extremes_have_distinct_colors() {
        let low = spectrum(255., 0., 0.);
        let mid = spectrum(0., 255., 0.);
        let high = spectrum(0., 0., 255.);
        assert_ne!(low, mid);
        assert_ne!(mid, high);
        assert_ne!(low, high);
    }
}
