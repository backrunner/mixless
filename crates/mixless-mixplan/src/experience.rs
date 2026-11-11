//! Playback residence follows audible envelopes, not a muted deck's playhead.
use mixless_protocol::MixPlan;
mod continuity;
mod entry;
mod signal;
pub(crate) use continuity::{continuity, minimum_bar_foreground_retention};
pub(crate) use entry::arrival_debt;
pub use entry::established_entry;
pub(crate) use signal::Evidence;
#[cfg(all(test, feature = "trace"))]
pub(crate) use signal::Scope;
#[cfg(test)]
#[path = "experience/tests.rs"]
mod content_tests;

pub(crate) fn terminal_handoff(
    p: &MixPlan,
    a: &mixless_protocol::TrackAnalysis,
    b: &mixless_protocol::TrackAnalysis,
) -> bool {
    let (exit, overlap) = continuity(p, a, b).map_or_else(
        || (p.t_out_a, overlap_bars(p, a, b)),
        |c| (c.last_out_sec, c.overlap_bars),
    );
    exit >= (a.duration_sec * 0.92).min(a.duration_sec - 8.) && overlap < 4.
}

/// Order repair must see a missing foreground even when occasional attacks
/// interrupt its sample-level gaps. Combine the existing whole-bar missing
/// time budget with the arranger's 6 dB foreground budget. Scattered short
/// dips with a retained bar, and isolated source impacts, do not qualify.
pub(crate) fn needs_repair(
    p: &MixPlan,
    a: &mixless_protocol::TrackAnalysis,
    b: &mixless_protocol::TrackAnalysis,
) -> bool {
    terminal_handoff(p, a, b)
        || continuity(p, a, b).is_some_and(|c| {
            c.longest_foreground_gap_bars >= 1.
                || (c.foreground_gap_bars >= 1.
                    && minimum_bar_foreground_retention(p, a, b).is_some_and(|r| r < 0.25))
        })
}

fn gain(p: &MixPlan, incoming: bool, part: usize, u: f32) -> f32 {
    let x = (p.lanes.xfader.sample(u) + 1.) * 0.5;
    let (fader, level, stem) = if incoming {
        (
            x,
            &p.lanes.gain_b,
            p.stem_mix.as_ref().map(|s| &s.incoming[part]),
        )
    } else {
        (
            1. - x,
            &p.lanes.gain_a,
            p.stem_mix.as_ref().map(|s| &s.outgoing[part]),
        )
    };
    fader.clamp(0., 1.).sqrt()
        * 10f32.powf(level.sample(u) / 20.)
        * stem.map_or(1., |s| s.sample(u))
}

/// The same current-transition cost belongs in pruning, shortlists and order.
/// A high raw cut score must not evict an overlap before lookahead sees it.
pub(crate) fn merit(
    p: &MixPlan,
    a: &mixless_protocol::TrackAnalysis,
    b: &mixless_protocol::TrackAnalysis,
) -> f32 {
    let Some(s) = &p.summary else {
        return f32::NEG_INFINITY;
    };
    s.score
        - if terminal_handoff(p, a, b) { 0.15 } else { 0. }
        - if s.length_bars < 2 && s.strategy == mixless_protocol::StrategyId::DryCut {
            0.1
        } else {
            0.
        }
}

/// The start of the final, established incoming passage. Muted playback and
/// a lone incoming drum layer cannot satisfy the next track's owed peak.
/// Compare with the final trim so recording gain does not move this point.
fn control_entry(p: &MixPlan) -> f32 {
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    if n < 1. {
        return p.t_in_b;
    }
    let reference = 10f32.powf(p.lanes.gain_b.sample(n) / 20.);
    let count = (n * 16.).ceil() as usize;
    let last_hidden = (0..count).rev().find(|i| {
        let u = *i as f32 / 16.;
        [0, 1, 2, 3]
            .into_iter()
            .any(|s| gain(p, true, s, u) < reference * 0.9)
    });
    last_hidden
        .map_or(p.t_in_b, |i| {
            p.source_seconds_at(true, (i + 1) as f32 / 16.)
        })
        .clamp(p.t_in_b, p.t_end_b)
}

/// When the outgoing melodic foreground starts leaving, even if a residual
/// voice or drum stem keeps the nominal transition running much longer.
pub(crate) fn foreground_exit(p: &MixPlan) -> f32 {
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    if n < 1. {
        return p.t_out_a;
    }
    let reference = gain(p, false, 2, 0.);
    (0..=(n * 16.) as usize)
        .map(|i| i as f32 / 16.)
        .find(|u| gain(p, false, 2, *u) < reference * 0.7)
        .map_or(p.t_out_a, |u| p.source_seconds_at(false, u))
}

/// Audible participation in outgoing master bars, including the final EQ,
/// filters and delayed source launch. A long reserved window is not overlap.
/// Loops and missing source clocks retain the nominal estimate, since this
/// predictor cannot certify the repeated source's participation.
pub(crate) fn overlap_bars(
    p: &MixPlan,
    a: &mixless_protocol::TrackAnalysis,
    b: &mixless_protocol::TrackAnalysis,
) -> f32 {
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    if n < 4.
        || p.outgoing_source.nodes.is_empty()
        || p.incoming_source.nodes.is_empty()
        || p.lanes.loop_a.is_some()
        || p.lanes.loop_b.is_some()
    {
        return n;
    }
    let (Some(ra), Some(rb)) = (
        crate::level::reference_power(a),
        crate::level::reference_power(b),
    ) else {
        return n;
    };
    overlap_with_reference(p, a, b, [ra, rb])
}

/// Reuse the same immutable source references while fitting multiple routes.
pub(crate) fn overlap_with_reference(
    p: &MixPlan,
    a: &mixless_protocol::TrackAnalysis,
    b: &mixless_protocol::TrackAnalysis,
    reference: [f32; 2],
) -> f32 {
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    let [ra, rb] = reference;
    let mut audible = 0.;
    for i in 0..(n * 8.) as usize {
        let u = (i as f32 + 0.5) / 8.;
        let heard = crate::level::deck_power(p, a, b, u);
        let normalized = [heard[0] / ra, heard[1] / rb];
        let total = normalized[0] + normalized[1];
        // The fitter uses the same -20 dB participation floor. Normalizing
        // each source removes mastering gain; tiny fade tails earn no time.
        if normalized.iter().all(|v| *v > 0.01 && *v > total * 0.01) {
            audible += 0.125;
        }
    }
    audible
}

/// The first master bar where B is heard louder than A, both relative to
/// their own reference power. With less than a bar of audible overlap, this
/// is where the listener actually hears the cut.
pub(crate) fn switch_bar(
    p: &MixPlan,
    a: &mixless_protocol::TrackAnalysis,
    b: &mixless_protocol::TrackAnalysis,
) -> Option<f32> {
    let n = p.summary.as_ref()?.length_bars as f32;
    let (ra, rb) = (
        crate::level::reference_power(a)?,
        crate::level::reference_power(b)?,
    );
    (0..=(n * 16.) as usize).map(|i| i as f32 / 16.).find(|u| {
        let heard = crate::level::deck_power(p, a, b, *u);
        heard[1] / rb > heard[0] / ra
    })
}

/// A nominally long plan whose decks share under a bar of audible time is a
/// cut. It needs the cut's own guards where that cut is actually heard: the
/// outgoing voice released, no outgoing peak severed, and an incoming phrase
/// entrance outside the middle of a peak.
pub(crate) fn genuine(p: &MixPlan, ctx: &crate::PlanContext<'_>) -> bool {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let n = p.summary.as_ref().map_or(0., |s| s.length_bars as f32);
    if n < 4.
        || crate::constraints::user_range(ctx.cues_out, a.sample_rate).is_some()
        || crate::constraints::user_range(ctx.cues_in, b.sample_rate).is_some()
        || overlap_bars(p, a, b) >= 1.
    {
        return true;
    }
    let Some(u) = switch_bar(p, a, b) else {
        return true;
    };
    // The end of the plan is its nominal boundary, already checked as a cut.
    if u >= n - 0.25 {
        return true;
    }
    let (sa, sb) = (p.outgoing_source.sample(u), p.incoming_source.sample(u));
    let voice = p.stem_mix.as_ref().map_or(1., |s| {
        s.outgoing[mixless_protocol::StemKind::Vocals.index()].sample(u)
    });
    (voice < 0.1 || crate::continuity::voice_released(a, sa))
        && !crate::drops::inside_peak(a, sa)
        && !crate::drops::inside_peak(b, sb)
        && crate::vocals::safe_entry(b, sb)
        && crate::phrasing::boundary_quality(b, sb) >= 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{MixPlanSummary, Polyline, StemMix};

    #[test]
    fn fallback_without_a_warp_curve_uses_its_rate_and_source_origin() {
        let mut p = MixPlan {
            summary: Some(MixPlanSummary {
                pair: (mixless_protocol::TrackId(1), mixless_protocol::TrackId(2)),
                strategy: mixless_protocol::StrategyId::FallbackSwapFilter,
                score: 0.4,
                used_fallback: true,
                length_bars: 1,
            }),
            t_in_a: 120.,
            t_out_a: 124.,
            t_in_b: 10.,
            t_end_b: 14.,
            clock: Polyline {
                nodes: vec![(0., 0.), (1., 4.)],
            },
            ..Default::default()
        };
        p.lanes.xfader = Polyline {
            nodes: vec![(0., -1.), (1., 1.)],
        };
        p.lanes.rate_a = Polyline::constant(1.);
        p.lanes.rate_b = Polyline::constant(1.);
        assert!(foreground_exit(&p) > 121.);
        assert!(control_entry(&p) > 13.);
    }

    #[test]
    fn a_muted_intro_and_drum_pickup_do_not_count_as_a_complete_entrance() {
        let mut p = MixPlan {
            summary: Some(MixPlanSummary {
                pair: (mixless_protocol::TrackId(1), mixless_protocol::TrackId(2)),
                strategy: mixless_protocol::StrategyId::StemSwap,
                score: 0.8,
                used_fallback: false,
                length_bars: 16,
            }),
            t_in_b: 20.,
            t_end_b: 52.,
            incoming_source: Polyline {
                nodes: vec![(0., 20.), (16., 52.)],
            },
            ..Default::default()
        };
        p.lanes.xfader = Polyline::constant(1.);
        p.lanes.gain_b = Polyline::constant(0.);
        let delayed = Polyline {
            nodes: vec![(0., 0.), (8., 0.), (12., 1.), (16., 1.)],
        };
        p.stem_mix = Some(StemMix {
            outgoing: std::array::from_fn(|_| Polyline::constant(0.)),
            incoming: std::array::from_fn(|s| {
                if s == 1 {
                    Polyline::constant(1.)
                } else {
                    delayed.clone()
                }
            }),
        });
        let entry = control_entry(&p);
        assert!(entry >= 43. && entry <= 44.);
        p.lanes.gain_b = Polyline::constant(-12.);
        assert_eq!(control_entry(&p), entry);
        p.stem_mix = None;
        assert_eq!(control_entry(&p), 20.);
    }

    #[test]
    fn a_reserved_window_and_a_delayed_launch_do_not_earn_overlap() {
        let a = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            32,
            0.8,
            0.2,
        );
        let b = crate::tests::track(
            2,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Intro,
            32,
            0.8,
            0.2,
        );
        let mut p = MixPlan {
            summary: Some(MixPlanSummary {
                length_bars: 16,
                pair: (a.track_id, b.track_id),
                strategy: mixless_protocol::StrategyId::PhraseBlend,
                score: 0.8,
                used_fallback: false,
            }),
            outgoing_source: Polyline {
                nodes: vec![(0., 32.), (16., 64.)],
            },
            incoming_source: Polyline {
                nodes: vec![(0., 0.), (16., 32.)],
            },
            ..Default::default()
        };
        p.lanes.xfader.nodes = vec![(0., -1.), (15., -1.), (16., 1.)];
        for filter in [&mut p.lanes.filter_a, &mut p.lanes.filter_b] {
            filter.hp_hz = Polyline::constant(20.);
            filter.lp_hz = Polyline::constant(20000.);
        }
        assert!(overlap_bars(&p, &a, &b) < 2.);
        p.lanes.xfader.nodes = vec![(0., -1.), (16., 1.)];
        assert!(overlap_bars(&p, &a, &b) > 12.);
        p.incoming_start_bar = 15.;
        assert!(overlap_bars(&p, &a, &b) < 2.);
        p.incoming_start_bar = 0.;
        p.lanes.eq_b.low = Polyline::constant(-40.);
        p.lanes.eq_b.mid = Polyline::constant(-40.);
        p.lanes.eq_b.high = Polyline::constant(-40.);
        assert_eq!(overlap_bars(&p, &a, &b), 0.);
    }
}
