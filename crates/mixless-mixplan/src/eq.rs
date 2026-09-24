//! Conservative tonal matching from the analyzed low/mid/high band energies.
//! There is no universal target curve: use robust recording/section profiles,
//! remove only excess energy, and never amplify an intentionally absent band.
use crate::{PerformanceLane, PerformanceMove, PlanContext};
use mixless_protocol::{EqBand, MixPlan, Polyline, TrackAnalysis};

fn profile(t: &TrackAnalysis, start: f32, end: f32) -> Option<[f32; 3]> {
    let mut levels: Vec<_> = t
        .bars
        .iter()
        .filter(|b| b.rms > 0.001)
        .map(|b| b.rms)
        .collect();
    levels.sort_by(f32::total_cmp);
    let reference = *levels.get(levels.len() * 3 / 4)?;
    let mut bands: [Vec<f32>; 3] = Default::default();
    for bar in t
        .bars
        .iter()
        .filter(|b| b.start_sec < end && b.end_sec > start && b.rms >= reference * 0.25)
    {
        let level = 10.
            * [bar.low_db, bar.mid_db, bar.high_db]
                .iter()
                .map(|db| 10f32.powf(db / 10.))
                .sum::<f32>()
                .max(1e-12)
                .log10();
        for (i, db) in [bar.low_db, bar.mid_db, bar.high_db]
            .into_iter()
            .enumerate()
        {
            if db.is_finite() && db > -90. {
                bands[i].push(db - level);
            }
        }
    }
    if bands.iter().any(|b| b.len() < 2) {
        return None;
    }
    Some(bands.map(|mut b| {
        b.sort_by(f32::total_cmp);
        b[b.len() / 2]
    }))
}

fn excess(local: [f32; 3], reference: [f32; 3], max: f32) -> [f32; 3] {
    let mut cuts = std::array::from_fn(|i| -((local[i] - reference[i] - 3.) * 0.25).clamp(0., max));
    // A joint budget prevents several individually modest cuts hollowing out
    // the entire signal. Values are dB, so attenuation adds conservatively.
    let sum: f32 = cuts.iter().sum::<f32>().abs();
    if sum > max * 1.5 {
        for v in &mut cuts {
            *v *= max * 1.5 / sum;
        }
    }
    cuts
}

pub(crate) fn solo(t: &TrackAnalysis, from: f32, until: f32) -> Vec<PerformanceMove> {
    let Some(reference) = profile(t, 0., t.duration_sec) else {
        return vec![];
    };
    let end = until.min(t.duration_sec) - 0.5;
    let start = from + 0.5;
    if end - start < 8. {
        return vec![];
    }
    let sections: Vec<_> = t
        .sections
        .iter()
        .map(|s| {
            (
                s.start_sec,
                s.end_sec,
                profile(t, s.start_sec, s.end_sec).map_or([0.; 3], |p| excess(p, reference, 1.5)),
            )
        })
        .collect();
    let count = ((end - start) / 0.25).ceil() as usize;
    let dt = (end - start) / count as f32;
    let mut curves = [vec![], vec![], vec![]];
    for i in 0..=count {
        let sec = start + i as f32 * dt;
        let target = sections
            .iter()
            .find(|(lo, hi, _)| sec >= *lo && sec < *hi)
            .map_or([0.; 3], |(_, _, db)| *db);
        for band in 0..3 {
            curves[band].push((
                sec,
                if i == 0 || i == count {
                    0.
                } else {
                    target[band]
                },
            ));
        }
    }
    let slew = 0.15 * dt;
    let mut moves = vec![];
    for (band, mut nodes) in [EqBand::Low, EqBand::Mid, EqBand::High]
        .into_iter()
        .zip(curves)
    {
        for i in 1..nodes.len() - 1 {
            nodes[i].1 = nodes[i]
                .1
                .clamp(nodes[i - 1].1 - slew, nodes[i - 1].1 + slew);
        }
        for i in (0..nodes.len() - 1).rev() {
            nodes[i].1 = nodes[i]
                .1
                .clamp(nodes[i + 1].1 - slew, nodes[i + 1].1 + slew);
        }
        if nodes.iter().any(|(_, v)| *v < -0.15) {
            moves.push(PerformanceMove {
                lane: PerformanceLane::Eq(band),
                start_sec: start,
                end_sec: end,
                curve: Polyline { nodes },
                label: "Section tone",
            });
        }
    }
    moves
}

pub(crate) fn arrange(ctx: &PlanContext<'_>, plan: &mut MixPlan) {
    let Some(summary) = &plan.summary else { return };
    let n = summary.length_bars as f32;
    let begin = plan.incoming_start_bar;
    let duration = plan.clock.sample(n) - plan.clock.sample(begin);
    if duration < 8. {
        return;
    }
    let (Some(a), Some(b)) = (
        profile(ctx.outgoing, plan.t_in_a, plan.t_out_a),
        profile(ctx.incoming, plan.t_in_b, plan.t_end_b),
    ) else {
        return;
    };
    let max = (duration * 0.15 / std::f32::consts::PI).min(2.);
    for (eq, cuts) in [
        (&mut plan.lanes.eq_a, excess(a, b, max)),
        (&mut plan.lanes.eq_b, excess(b, a, max)),
    ] {
        for (line, cut) in [&mut eq.low, &mut eq.mid, &mut eq.high]
            .into_iter()
            .zip(cuts)
        {
            if cut >= -0.05 {
                continue;
            }
            let mut points: Vec<_> = line.nodes.iter().map(|(u, _)| *u).collect();
            points.extend((0..=64).map(|i| begin + (n - begin) * i as f32 / 64.));
            points.sort_by(f32::total_cmp);
            points.dedup_by(|a, b| (*a - *b).abs() < 0.0001);
            line.nodes = points
                .into_iter()
                .map(|u| {
                    let db = line.sample(u);
                    let x = ((plan.clock.sample(u) - plan.clock.sample(begin)) / duration)
                        .clamp(0., 1.);
                    let shade = (std::f32::consts::PI * x).sin().powi(2);
                    (u, db + cut * shade * ((db + 12.) / 12.).clamp(0., 1.))
                })
                .collect();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn section_compensation_is_gain_invariant_cut_only_and_slow() {
        let base = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Unknown,
            64,
            0.8,
            0.2,
        );
        let mut previous: Option<Vec<PerformanceMove>> = None;
        for gain in [0.25f32, 1., 2.] {
            let mut t = base.clone();
            t.sections = [0., 32., 64., 128.]
                .windows(2)
                .map(|w| mixless_protocol::Section {
                    start_sec: w[0],
                    end_sec: w[1],
                    label: mixless_protocol::SectionLabel::Unknown,
                })
                .collect();
            for b in &mut t.bars {
                b.rms *= gain;
                b.low_db += 20. * gain.log10();
                b.mid_db += 20. * gain.log10();
                b.high_db += 20. * gain.log10();
                if b.start_sec >= 32. && b.start_sec < 64. {
                    b.high_db += 12.;
                }
            }
            let moves = solo(&t, 0., 128.);
            assert_eq!(moves.len(), 1);
            assert_eq!(moves[0].lane, PerformanceLane::Eq(EqBand::High));
            let nodes = &moves[0].curve.nodes;
            assert_eq!(nodes[0].1, 0.);
            assert_eq!(nodes.last().unwrap().1, 0.);
            assert!(nodes.iter().any(|(_, v)| *v < -1.));
            assert!(nodes.iter().all(|(_, v)| (-1.5..=0.).contains(v)));
            for w in nodes.windows(2) {
                assert!((w[1].1 - w[0].1).abs() / (w[1].0 - w[0].0) <= 0.15001);
            }
            if let Some(previous) = &previous {
                for (a, b) in previous[0].curve.nodes.iter().zip(nodes) {
                    assert!((a.1 - b.1).abs() < 1e-5);
                }
            }
            previous = Some(moves);
        }
        assert!(solo(&base, 0., 128.).is_empty());
        let mut missing = base;
        missing.bars.clear();
        assert!(solo(&missing, 0., 128.).is_empty());
    }
}
