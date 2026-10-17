//! Check the audible result, including gain, crossfader, EQ and stems.
use crate::PlanContext;
use mixless_protocol::{MixPlan, StemKind};

pub(crate) fn preserved(p: &MixPlan, ctx: &PlanContext<'_>) -> bool {
    if crate::constraints::user_range(ctx.cues_out, ctx.outgoing.sample_rate).is_some() {
        return true;
    }
    let Some(summary) = &p.summary else {
        return true;
    };
    let n = summary.length_bars as f32;
    if n < 2. || p.lanes.loop_a.is_some() {
        return true;
    }
    let a = ctx.outgoing;
    let peaks = crate::drops::peaks(a);
    let voice_gain = |u: f32| {
        let l = &p.lanes;
        let fader = ((1. - l.xfader.sample(u)) * 0.5).clamp(0., 1.).sqrt();
        let stem = p
            .stem_mix
            .as_ref()
            .map_or(1., |s| s.outgoing[StemKind::Vocals.index()].sample(u));
        let lp = l.filter_a.lp_hz.sample(u).max(20.);
        let hp = l.filter_a.hp_hz.sample(u).max(20.);
        fader * stem * 10f32.powf((l.gain_a.sample(u) + l.eq_a.mid.sample(u)) / 20.)
            / ((1. + (600. / lp).powi(4)) * (1. + (hp / 600.).powi(4))).sqrt()
    };
    let mut prior = voice_gain(0.);
    if prior < 0.7 && crate::continuity::voice_active(a, p.outgoing_source.sample(0.)) > 0.35 {
        return false;
    }
    let mut power_window = [[0f32; 2]; 16];
    let mut count = 0usize;
    for i in 1..=(n * 16.) as usize {
        let u = i as f32 / 16.;
        let sec = p.outgoing_source.sample(u);
        let current = voice_gain(u);
        // A low syllable or held tail is still part of the line. Check the
        // whole audible descent, not just its crossing of a -3 dB threshold.
        let fading = current < prior - 0.0001 && current < 0.95 && (current > 0.01 || prior >= 0.7);
        if fading && sec < a.duration_sec - 0.001 {
            let sung = if let Some(stems) = &a.stems {
                stems.frame_at(sec).is_none_or(|f| {
                    let audible = f.rms[0].powi(2) > (f.rms[1].powi(2) + f.rms[2].powi(2)) * 0.0001;
                    audible
                        && (f.vocal_activity >= 0.2 || stems.note_crossing(sec, StemKind::Vocals))
                })
            } else {
                crate::continuity::voice_active(a, sec) > 0.35
            };
            if sung {
                return false;
            }
        }
        prior = current;
        if u >= n - 0.25 {
            continue;
        }
        if peaks
            .iter()
            .any(|p0| sec > p0.0 + 0.05 && sec < p0.1 - 0.05)
        {
            let reference = 10f32.powf(crate::level::unmixed(a, sec) / 10.);
            let heard = 10f32.powf(crate::level::at(p, a, ctx.incoming, u) / 10.);
            power_window[count % 16] = [reference, heard];
            count += 1;
            // A single kick or one short pause is not sustained energy loss.
            // Compare a complete local bar of power, not isolated samples.
            if count >= 16 {
                let sum: [f32; 2] =
                    std::array::from_fn(|d| power_window.iter().map(|p| p[d]).sum());
                if sum[1] < sum[0] * 0.5 {
                    return false;
                }
            }
        } else {
            count = 0;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{MixPlanSummary, Polyline, Section, SectionLabel as S, StrategyId};
    #[test]
    fn a_peak_can_exchange_for_equal_energy_but_cannot_fade_into_a_quiet_intro() {
        let mut a = crate::tests::track(1, 120., "8A", S::Drop, 64, 0.9, 0.2);
        a.sections = vec![
            Section {
                start_sec: 0.,
                end_sec: 64.,
                label: S::Drop,
            },
            Section {
                start_sec: 64.,
                end_sec: 128.,
                label: S::Outro,
            },
        ];
        let mut b = crate::tests::track(2, 120., "8A", S::Intro, 64, 0.9, 0.2);
        let mut p = MixPlan {
            summary: Some(MixPlanSummary {
                pair: (a.track_id, b.track_id),
                strategy: StrategyId::BassSwap,
                score: 1.,
                used_fallback: false,
                length_bars: 16,
            }),
            t_in_a: 32.,
            t_out_a: 64.,
            t_end_b: 32.,
            outgoing_source: Polyline {
                nodes: vec![(0., 32.), (16., 64.)],
            },
            incoming_source: Polyline {
                nodes: vec![(0., 0.), (16., 32.)],
            },
            ..Default::default()
        };
        p.lanes.xfader = Polyline {
            nodes: vec![(0., -1.), (16., 1.)],
        };
        p.lanes.gain_a = Polyline::constant(0.);
        p.lanes.gain_b = Polyline::constant(0.);
        for filter in [&mut p.lanes.filter_a, &mut p.lanes.filter_b] {
            filter.lp_hz = Polyline::constant(20000.);
            filter.hp_hz = Polyline::constant(20.);
        }
        let check = |b: &mixless_protocol::TrackAnalysis| {
            preserved(
                &p,
                &PlanContext {
                    outgoing: &a,
                    incoming: b,
                    cues_out: &[],
                    cues_in: &[],
                    offset_a: Default::default(),
                    offset_b: Default::default(),
                },
            )
        };
        assert!(check(&b));
        for bar in &mut b.bars {
            bar.rms *= 0.1;
        }
        assert!(!check(&b));
    }
}
