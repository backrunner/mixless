//! The short-cut path must not skip the foreground comparison applied to blends.
use mixless_protocol::{MixPlan, StrategyId, TrackAnalysis};

pub(super) fn cost(p: &MixPlan, a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    if p.summary
        .as_ref()
        .is_none_or(|s| s.strategy != StrategyId::DryCut)
        || p.send_fx.is_some()
        || p.lanes.loop_a.is_some()
        || p.lanes.loop_b.is_some()
    {
        return 0.;
    }
    let following = |t: &TrackAnalysis, at: f32| -> Option<f32> {
        let reference = crate::level::reference_power(t)?;
        let g = crate::grid::Grid(t);
        let beat = g.beat(at);
        let mut sum = 0.;
        for i in 0..8 {
            let sec = g.sec(beat + (i as f32 + 0.5) * g.meter() / 8.);
            sum += crate::level::unmixed_foreground(t, sec)? / reference;
        }
        Some(sum / 8.)
    };
    let Some((expected, replacement)) = following(a, p.t_out_a).zip(following(b, p.t_in_b)) else {
        return 0.;
    };
    if expected < 0.01 {
        return 0.;
    }
    // Same 6 dB foreground allowance and bounded weight as the sustained-blend
    // test. EOF/natural releases have no expected foreground to preserve.
    let loss = (10. * (expected / replacement.max(1e-12)).log10() - 6.).max(0.);
    0.18 * (loss / 6.).min(1.)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::MixPlanSummary;

    #[test]
    fn drums_do_not_replace_a_live_foreground_but_a_natural_tail_can_cut() {
        let mut a = crate::vocals::chop_tests::track(true);
        for f in &mut a.stems.as_mut().unwrap().frames {
            f.band_db = [[-90.; 3], [-20.; 3], [-20.; 3]];
        }
        let mut b = a.clone();
        for f in &mut b.stems.as_mut().unwrap().frames {
            f.band_db = [[-90.; 3], [-17.; 3], [-40.; 3]];
        }
        let p = MixPlan {
            t_out_a: 8.,
            t_in_b: 0.,
            summary: Some(MixPlanSummary {
                pair: (a.track_id, b.track_id),
                score: 0.5,
                used_fallback: false,
                length_bars: 0,
                strategy: StrategyId::DryCut,
            }),
            ..Default::default()
        };
        assert!(cost(&p, &a, &b) > 0.1);
        assert_eq!(cost(&p, &a, &a), 0.);
        for scale in [0.1_f32, 1., 10.] {
            let (mut x, mut y) = (a.clone(), b.clone());
            for t in [&mut x, &mut y] {
                for f in &mut t.stems.as_mut().unwrap().frames {
                    for db in f.band_db.iter_mut().flatten() {
                        *db += 20. * scale.log10();
                    }
                }
            }
            assert!((cost(&p, &x, &y) - cost(&p, &a, &b)).abs() < 1e-5);
        }
        for f in &mut a.stems.as_mut().unwrap().frames {
            if f.start_sec >= 8. {
                f.band_db[2] = [-90.; 3];
            }
        }
        assert_eq!(cost(&p, &a, &b), 0.);
        let mut eof = p.clone();
        eof.t_out_a = a.duration_sec;
        assert_eq!(cost(&eof, &a, &b), 0.);
        a.stems = None;
        assert_eq!(
            cost(&p, &a, &b),
            0.,
            "missing stems are unknown, not a blanket veto"
        );
    }
}
