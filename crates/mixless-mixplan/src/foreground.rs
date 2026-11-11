//! Check the audible result, including gain, crossfader, EQ and stems.
use crate::PlanContext;
use mixless_protocol::{MixPlan, StemKind};

#[cfg(test)]
#[path = "foreground/regressions.rs"]
mod regressions;

/// Uncertain source association affects preference, never the availability of
/// an otherwise safe mix. Inspect the audible voice exchange, not the start of
/// a drum-only preload or the residual deck's final stop.
pub(crate) fn uncertainty(p: &MixPlan, ctx: &PlanContext<'_>) -> f32 {
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    let mut risk = 0f32;
    for (side, track, source, gain, eq, filter) in [
        (
            0,
            ctx.outgoing,
            &p.outgoing_source,
            &p.lanes.gain_a,
            &p.lanes.eq_a,
            &p.lanes.filter_a,
        ),
        (
            1,
            ctx.incoming,
            &p.incoming_source,
            &p.lanes.gain_b,
            &p.lanes.eq_b,
            &p.lanes.filter_b,
        ),
    ] {
        let boundary = if side == 0 { p.t_out_a } else { p.t_in_b };
        let sec = if n < 2. || source.nodes.is_empty() {
            boundary
        } else {
            let reference = gain.sample(if side == 0 { 0. } else { n });
            (0..=(n * 16.) as usize)
                .map(|i| i as f32 / 16.)
                .filter(|u| side == 0 || *u >= p.incoming_start_bar)
                .find(|u| {
                    let x = ((1. + p.lanes.xfader.sample(*u)) * 0.5).clamp(0., 1.);
                    let stem = p.stem_mix.as_ref().map_or(1., |s| {
                        if side == 0 {
                            s.outgoing[0].sample(*u)
                        } else {
                            s.incoming[0].sample(*u)
                        }
                    });
                    let amplitude = (if side == 0 { 1. - x } else { x }).sqrt()
                        * stem
                        * 10f32.powf((gain.sample(*u) - reference + eq.mid.sample(*u)) / 20.)
                        / ((1. + (600. / filter.lp_hz.sample(*u).max(20.)).powi(4))
                            * (1. + (filter.hp_hz.sample(*u).max(20.) / 600.).powi(4)))
                        .sqrt();
                    if side == 0 {
                        amplitude < 0.5
                    } else {
                        amplitude > 0.5
                    }
                })
                .map_or(boundary, |u| source.sample(u))
        };
        risk = risk.max(crate::vocals::continuation(track, sec).cost());
    }
    risk.min(0.05)
}

pub(crate) fn preserved(p: &MixPlan, ctx: &PlanContext<'_>) -> bool {
    let outgoing_override = crate::constraints::user_range(ctx.cues_out, ctx.outgoing.sample_rate)
        .is_some()
        || p.lanes.loop_a.is_some();
    let Some(summary) = &p.summary else {
        return true;
    };
    let n = summary.length_bars as f32;
    if n < 2. {
        return (outgoing_override || !crate::vocals::continues(ctx.outgoing, p.t_out_a))
            && (crate::constraints::user_range(ctx.cues_in, ctx.incoming.sample_rate).is_some()
                || !crate::vocals::continues(ctx.incoming, p.t_in_b));
    }
    if !crate::melody::preserved(p, ctx) {
        return false;
    }
    // The incoming singer must enter at a complete breath too. A safe source
    // IN alone is insufficient when a later stem/fader envelope hides the
    // beginning of the word and reveals its middle.
    let incoming_voice = |u: f32| {
        let l = &p.lanes;
        let fader = ((1. + l.xfader.sample(u)) * 0.5).clamp(0., 1.).sqrt();
        let stem = p
            .stem_mix
            .as_ref()
            .map_or(1., |s| s.incoming[StemKind::Vocals.index()].sample(u));
        let reference = l.gain_b.sample(n);
        let lp = l.filter_b.lp_hz.sample(u).max(20.);
        let hp = l.filter_b.hp_hz.sample(u).max(20.);
        fader * stem * 10f32.powf((l.gain_b.sample(u) - reference + l.eq_b.mid.sample(u)) / 20.)
            / ((1. + (600. / lp).powi(4)) * (1. + (hp / 600.).powi(4))).sqrt()
    };
    // Once the whole singer is present (the same -3 dB presence floor used
    // below), a small level restoration cannot reveal a missing word. Fades
    // from silence still pass through the guarded range before this point.
    let launch = p.incoming_start_bar.clamp(0., n);
    let mut previous = incoming_voice(launch);
    if previous > 0.01
        && crate::constraints::user_range(ctx.cues_in, ctx.incoming.sample_rate).is_none()
        && !crate::vocals::safe_entry(ctx.incoming, p.incoming_source.sample(launch))
    {
        return false;
    }
    for i in (launch * 32.).floor() as usize + 1..=(n * 32.) as usize {
        let u = i as f32 / 32.;
        let current = incoming_voice(u);
        if current > previous + 0.0001 && current > 0.01 && previous < 0.7 {
            let sec = p.incoming_source.sample(u);
            let sung = ctx.incoming.stems.as_ref().map_or_else(
                || crate::continuity::voice_active(ctx.incoming, sec) > 0.35,
                |stems| {
                    stems.frame_at(sec).is_none_or(|f| {
                        f.rms[0].powi(2) > (f.rms[1].powi(2) + f.rms[2].powi(2)) * 0.0001
                            && (f.vocal_activity >= 0.2
                                || crate::vocals::held_note(ctx.incoming, sec))
                    })
                },
            );
            if (sung || crate::vocals::continues(ctx.incoming, sec))
                && crate::vocals::pattern::at(ctx.incoming, sec).is_none()
            {
                return false;
            }
        }
        previous = current;
    }
    // A manual outgoing cue or its loop has authority over that source only.
    // The incoming singer still needs an intact audible entrance.
    if outgoing_override {
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
    // A continuing drum layer does not own the old melodic foreground.
    let bed_gain = |u: f32| {
        let l = &p.lanes;
        let fader = ((1. - l.xfader.sample(u)) * 0.5).clamp(0., 1.).sqrt();
        let stem = p
            .stem_mix
            .as_ref()
            .map_or(1., |s| s.outgoing[StemKind::Instruments.index()].sample(u));
        fader * stem * 10f32.powf(l.gain_a.sample(u) / 20.)
    };
    let mut prior = voice_gain(0.);
    if prior < 0.7 && crate::continuity::voice_active(a, p.outgoing_source.sample(0.)) > 0.35 {
        return false;
    }
    let mut power_window = [[0f32; 2]; 16];
    let references =
        crate::level::reference_power(a).zip(crate::level::reference_power(ctx.incoming));
    let mut foreground_window = [[0f32; 2]; 16];
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
                    audible && (f.vocal_activity >= 0.2 || crate::vocals::held_note(a, sec))
                })
            } else {
                crate::continuity::voice_active(a, sec) > 0.35
            };
            if (sung || crate::vocals::continues(a, sec))
                && crate::vocals::pattern::at(a, sec).is_none()
            {
                return false;
            }
        }
        // A voice muted in one breath must not drop the next whole line while
        // A's peak arrangement is still plainly heard. That reads as a lyric
        // edit, and a total-power check cannot see it under a loud bed.
        // An incoming singer who has taken the floor is a vocal exchange.
        if current < 0.1
            && sec < a.duration_sec - 0.001
            && bed_gain(u) > 0.5
            && incoming_voice(u) < 0.5
        {
            let in_peak = peaks
                .iter()
                .any(|p0| sec > p0.0 + 0.05 && sec < p0.1 - 0.05);
            let sung = a.stems.as_ref().is_some_and(|stems| {
                stems.frame_at(sec).is_some_and(|f| {
                    f.rms[0].powi(2) > (f.rms[1].powi(2) + f.rms[2].powi(2)) * 0.01
                        && f.vocal_activity >= 0.5
                })
            });
            if in_peak && sung && crate::vocals::pattern::at(a, sec).is_none() {
                return false;
            }
        }
        prior = current;
        if let Some((ra, rb)) = references {
            if let Some((raw, heard)) = crate::level::foreground_power(p, a, ctx.incoming, u) {
                foreground_window[(i - 1) % 16] = [raw[0] / ra, heard[0] / ra + heard[1] / rb];
                if i >= 16 {
                    let average: [f32; 2] = std::array::from_fn(|d| {
                        foreground_window.iter().map(|v| v[d]).sum::<f32>() / 16.
                    });
                    // A real source break remains a break. Controls cannot
                    // remove a meaningful foreground for a whole local bar,
                    // even when loud drums hide the hole in total power.
                    if average[0] > 0.02 && average[1] < 0.02 && average[1] < average[0] * 0.125 {
                        return false;
                    }
                }
            }
        }
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
        let check = |p: &MixPlan, b: &mixless_protocol::TrackAnalysis| {
            preserved(
                p,
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
        assert!(check(&p, &b));
        let mut dip = p.clone();
        for width in [0.25, 2.] {
            for gain in [&mut dip.lanes.gain_a, &mut dip.lanes.gain_b] {
                gain.nodes = vec![
                    (0., 0.),
                    (7., 0.),
                    (7.0625, -6.),
                    (7. + width, -6.),
                    (7. + width + 0.0625, 0.),
                    (16., 0.),
                ];
            }
            assert_eq!(
                check(&dip, &b),
                width < 1.,
                "a short crossing may dip; a sustained loss still rejects the rendered plan"
            );
        }
        for bar in &mut b.bars {
            bar.rms *= 0.1;
        }
        assert!(!check(&p, &b));
    }
}
