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
    let window = plan.mix_windows()?;
    Some((window.outgoing, window.incoming))
}

#[cfg(test)]
pub fn rows(
    on: bool,
    tracks: &[Track],
    plans: &[Option<Arc<MixPlan>>],
    active: Option<&MixPlan>,
) -> Vec<Overlay> {
    rows_at(on, tracks, plans, active, None)
}

pub fn rows_at(
    on: bool,
    tracks: &[Track],
    plans: &[Option<Arc<MixPlan>>],
    active: Option<&MixPlan>,
    active_rows: Option<(usize, usize)>,
) -> Vec<Overlay> {
    let mut rows = vec![Overlay::default(); tracks.len()];
    if !on || tracks.is_empty() {
        return rows;
    }
    for (position, plan) in plans
        .iter()
        .enumerate()
        .filter_map(|(i, p)| Some((i, p.as_ref()?)))
    {
        if let Some((outgoing, incoming)) = ranges(plan) {
            let pair = plan.summary.as_ref().unwrap().pair;
            if !(0..tracks.len()).any(|i| (tracks[i].id, tracks[(i + 1) % tracks.len()].id) == pair)
            {
                continue;
            }
            let matches: Vec<_> = (0..tracks.len())
                .filter(|i| (tracks[*i].id, tracks[(*i + 1) % tracks.len()].id) == pair)
                .collect();
            // Repeated track IDs are separate occurrences. Only an aligned
            // sequence slot can disambiguate a repeated neighbour pair.
            let index = if matches.len() == 1 {
                matches.first().copied()
            } else {
                matches.into_iter().find(|i| *i == position)
            };
            if let Some(i) = index {
                rows[i].outgoing = Some(outgoing);
                rows[(i + 1) % tracks.len()].incoming = Some(incoming);
            }
        }
    }
    // Shuffle/late replanning can select a non-adjacent pair. The armed plan
    // takes priority over the nominal playlist preview on both source rows.
    if let Some((plan, (outgoing, incoming))) = active.and_then(|p| Some((p, ranges(p)?))) {
        let pair = plan.summary.as_ref().unwrap().pair;
        let neighbours: Vec<_> = (0..tracks.len())
            .filter(|i| (tracks[*i].id, tracks[(*i + 1) % tracks.len()].id) == pair)
            .collect();
        let selected = if let Some((a, b)) = active_rows.filter(|(a, b)| {
            *a < tracks.len() && *b < tracks.len() && (tracks[*a].id, tracks[*b].id) == pair
        }) {
            Some((a, b))
        } else if neighbours.len() == 1 {
            Some((neighbours[0], (neighbours[0] + 1) % tracks.len()))
        } else {
            let one = |id| {
                let mut found = tracks.iter().enumerate().filter(|(_, t)| t.id == id);
                let first = found.next()?.0;
                found.next().is_none().then_some(first)
            };
            one(pair.0).zip(one(pair.1))
        };
        for (i, (row, track)) in rows.iter_mut().zip(tracks).enumerate() {
            if active_rows.is_some() && selected.is_some_and(|(a, b)| i != a && i != b) {
                continue;
            }
            if track.id == pair.0 {
                row.outgoing = None;
            }
            if track.id == pair.1 {
                row.incoming = None;
            }
        }
        if let Some((a, b)) = selected {
            rows[a].outgoing = Some(outgoing);
            rows[b].incoming = Some(incoming);
        }
    }
    // A changed live entrance invalidates a forecasted exit before it. Never
    // draw two incompatible occurrences (including a playlist wrap) as if
    // they belonged to one pass through this track.
    for (row, track) in rows.iter_mut().zip(tracks) {
        if let (Some((_, entered)), Some((leaving, _))) = (row.incoming, row.outgoing) {
            if entered > leaving + 0.001 {
                if active
                    .and_then(|p| p.summary.as_ref())
                    .is_some_and(|s| s.pair.0 == track.id)
                {
                    row.incoming = None;
                } else {
                    row.outgoing = None;
                }
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
        let conflict = vec![Some(plan(1, 2, 40., 88.)), Some(plan(2, 3, 20., 60.)), None];
        let corrected = rows(true, &tracks, &conflict, None);
        assert_eq!(corrected[1].incoming, Some((8., 56.)));
        assert!(corrected[1].outgoing.is_none());
        let actual = plan(2, 3, 20., 60.);
        let corrected = rows(true, &tracks, &conflict, Some(&actual));
        assert!(corrected[1].incoming.is_none());
        assert_eq!(corrected[1].outgoing, Some((20., 60.)));
        let active = plan(1, 3, 42., 90.);
        let shuffled = rows(true, &tracks, &plans, Some(&active));
        assert_eq!(shuffled[0].outgoing, Some((42., 90.)));
        assert_eq!(shuffled[2].incoming, Some((8., 56.)));

        // A repeated ID is not permission to paint another occurrence's exit.
        let repeated = vec![
            tracks[0].clone(),
            tracks[1].clone(),
            tracks[0].clone(),
            tracks[1].clone(),
        ];
        let p = vec![
            Some(plan(1, 2, 40., 88.)),
            None,
            Some(plan(1, 2, 70., 118.)),
            None,
        ];
        let shown = rows(true, &repeated, &p, None);
        assert_eq!(shown[0].outgoing, Some((40., 88.)));
        assert_eq!(shown[2].outgoing, Some((70., 118.)));
        let ambiguous = rows(true, &repeated, &p, p[0].as_deref());
        assert!(ambiguous[0].outgoing.is_none() && ambiguous[2].outgoing.is_none());

        // An unrelated active pair cannot resurrect a removed forecast edge.
        let changed = vec![tracks[0].clone(), tracks[2].clone(), tracks[1].clone()];
        let shown = rows(true, &changed, &plans, Some(&active));
        assert!(shown[2].outgoing.is_none());
    }
}
