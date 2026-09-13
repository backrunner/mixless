//! Source-time previews of prepared plans; no analysis or database work in paint.
use gpui::{Bounds, Pixels, Window};
use mixless_protocol::{MixPlan, Track};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Overlay {
    pub incoming: Option<(f32, f32)>,
    pub outgoing: Option<(f32, f32)>,
}

fn ranges(plan: &MixPlan) -> Option<((f32, f32), (f32, f32))> {
    let n = plan.summary.as_ref()?.length_bars as f32;
    if plan.failure_reason.is_some() {
        return None;
    }
    // A cut has no musical overlap, even if its scheduler was armed earlier.
    let cut = plan.incoming_start_bar >= n;
    Some((
        (
            if cut {
                plan.t_out_a
            } else if plan.outgoing_source.nodes.is_empty() {
                plan.t_in_a + plan.clock.sample(plan.incoming_start_bar) * plan.outgoing_offset.rate
            } else {
                plan.outgoing_source.sample(plan.incoming_start_bar)
            },
            plan.t_out_a,
        ),
        // t_end_b accounts for loop repetitions; the unwrapped warp clock does not.
        (plan.t_in_b, if cut { plan.t_in_b } else { plan.t_end_b }),
    ))
}

pub fn rows(
    on: bool,
    tracks: &[Track],
    plans: &[Option<Arc<MixPlan>>],
    active: Option<&MixPlan>,
) -> Vec<Overlay> {
    let mut rows = vec![Overlay::default(); tracks.len()];
    if !on || tracks.is_empty() {
        return rows;
    }
    for i in 0..tracks.len() {
        let next = (i + 1) % tracks.len();
        let pair = (tracks[i].id, tracks[next].id);
        let plan = active
            .filter(|p| p.summary.as_ref().is_some_and(|s| s.pair == pair))
            .or_else(|| {
                plans
                    .get(i)?
                    .as_deref()
                    .filter(|p| p.summary.as_ref().is_some_and(|s| s.pair == pair))
            });
        if let Some((outgoing, incoming)) = plan.and_then(ranges) {
            rows[i].outgoing = Some(outgoing);
            rows[next].incoming = Some(incoming);
        }
    }
    // Shuffle/late replanning can select a non-adjacent pair. The armed plan
    // takes priority over the nominal playlist preview on both source rows.
    if let Some((plan, (outgoing, incoming))) = active.and_then(|p| Some((p, ranges(p)?))) {
        let pair = plan.summary.as_ref().unwrap().pair;
        for (row, track) in rows.iter_mut().zip(tracks) {
            if track.id == pair.0 {
                row.outgoing = Some(outgoing);
            }
            if track.id == pair.1 {
                row.incoming = Some(incoming);
            }
        }
    }
    rows
}

pub fn paint(overlay: Overlay, duration: f32, bounds: Bounds<Pixels>, window: &mut Window) {
    use super::{pf, quad_fill};
    let width = pf(bounds.size.width);
    if !duration.is_finite() || duration <= 0. || width <= 2. {
        return;
    }
    let left = pf(bounds.origin.x);
    let top = pf(bounds.origin.y);
    for (index, range) in [overlay.incoming, overlay.outgoing].into_iter().enumerate() {
        let Some((start, end)) = range else { continue };
        if !start.is_finite() || !end.is_finite() || end < start {
            continue;
        }
        let x = left + (start / duration * width).clamp(0., width - 1.);
        let right = left + (end / duration * width).clamp(0., width - 1.);
        let color = if index == 0 {
            gpui::rgb(0x63dacb)
        } else {
            gpui::rgb(0xe6a0f1)
        };
        let shade = if index == 0 {
            gpui::rgba(0x63dacb30)
        } else {
            gpui::rgba(0xe6a0f130)
        };
        let y = top
            + if index == 0 {
                0.
            } else {
                pf(bounds.size.height) / 2.
            };
        let h = pf(bounds.size.height) / 2.;
        quad_fill(window, x, y, (right - x).max(1.), h, shade);
        quad_fill(window, x, y, 1., h, color);
        quad_fill(window, right, y, 1., h, color);
        quad_fill(window, x, y + h - 2., (right - x).max(2.), 2., color);
        // Compact pixel labels stay legible over a dense waveform.
        let letters: &[[u8; 5]] = if index == 0 {
            &[[7, 2, 2, 2, 7], [5, 7, 7, 7, 5]]
        } else {
            &[[7, 5, 5, 5, 7], [5, 5, 5, 5, 7], [7, 2, 2, 2, 2]]
        };
        let label_width = letters.len() as f32 * 4. + 3.;
        let anchor = if index == 0 { x } else { right };
        let lx = anchor.min(left + width - label_width).max(left);
        quad_fill(window, lx, y, label_width, 7., gpui::rgb(0x111218));
        for (letter, rows) in letters.iter().enumerate() {
            for (row, bits) in rows.iter().enumerate() {
                for col in 0..3 {
                    if bits & (1 << (2 - col)) != 0 {
                        quad_fill(
                            window,
                            lx + 2. + letter as f32 * 4. + col as f32,
                            y + 1. + row as f32,
                            1.,
                            1.,
                            color,
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cut_preview_excludes_preparation_and_off_has_no_ranges() {
        let mut p = MixPlan::default();
        p.summary = Some(mixless_protocol::MixPlanSummary {
            pair: (mixless_protocol::TrackId(1), mixless_protocol::TrackId(2)),
            strategy: mixless_protocol::StrategyId::DropCut,
            score: 1.,
            used_fallback: false,
            length_bars: 4,
        });
        p.t_in_a = 12.;
        p.t_out_a = 20.;
        p.t_in_b = 30.;
        p.incoming_start_bar = 4.;
        assert_eq!(ranges(&p), Some(((20., 20.), (30., 30.))));
        assert!(rows(false, &[], &[Some(Arc::new(p))], None).is_empty());
    }

    #[test]
    fn toggles_reorders_and_active_shuffle_do_not_leave_stale_markers() {
        let tracks: Vec<Track> = (1..=3)
            .map(|id| Track {
                id: mixless_protocol::TrackId(id),
                path: String::new(),
                artwork_path: None,
                title: String::new(),
                artist: String::new(),
                album: None,
                duration_ms: 120000,
                isrc: None,
                bpm: None,
                key: None,
                camelot: None,
                analyzed: true,
                content_hash: String::new(),
            })
            .collect();
        let plan = |a, b, start, end| {
            Arc::new(MixPlan {
                summary: Some(mixless_protocol::MixPlanSummary {
                    pair: (mixless_protocol::TrackId(a), mixless_protocol::TrackId(b)),
                    strategy: mixless_protocol::StrategyId::BassSwap,
                    score: 1.,
                    used_fallback: false,
                    length_bars: 24,
                }),
                t_in_a: start,
                t_out_a: end,
                t_in_b: 8.,
                t_end_b: 56.,
                ..Default::default()
            })
        };
        let plans = vec![
            Some(plan(1, 2, 40., 88.)),
            Some(plan(2, 3, 60., 108.)),
            None,
        ];
        let visible = rows(true, &tracks, &plans, None);
        assert_eq!(visible[0].outgoing, Some((40., 88.)));
        assert_eq!(visible[1].incoming, Some((8., 56.)));
        assert_eq!(visible[1].outgoing, Some((60., 108.)));
        assert!(
            rows(false, &tracks, &plans, None)
                .iter()
                .all(|r| *r == Overlay::default())
        );
        let mut reversed = tracks.clone();
        reversed.reverse();
        assert!(
            rows(true, &reversed, &plans, None)
                .iter()
                .all(|r| *r == Overlay::default())
        );
        let active = plan(1, 3, 42., 90.);
        let shuffled = rows(true, &tracks, &plans, Some(&active));
        assert_eq!(shuffled[0].outgoing, Some((42., 90.)));
        assert_eq!(shuffled[2].incoming, Some((8., 56.)));
    }
}
