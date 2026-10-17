//! Tail effects on the echo send. A tail only ever carries the outgoing
//! track's last moment under the incoming one; it never replaces the dry
//! signal. An echo repeats on the outgoing beat, so it is used only when both
//! decks share a tempo; across a tempo gap only a rhythm-free reverb is used.
use crate::{PlanContext, grid::Grid, musical::feature};
use mixless_protocol::{FxKind, MixPlan, Polyline, SendFx};

/// Feedback level below which a repeat is inaudible under the incoming track.
const SILENT: f32 = 0.001;

pub(crate) fn echo(kind: FxKind, beats: f32, feedback: f32, bpm: f32) -> SendFx {
    let period = beats * 60. / bpm.max(40.);
    let repeats = SILENT.ln() / feedback.ln();
    SendFx {
        kind,
        beats,
        feedback,
        decay_seconds: 1.6,
        tail_seconds: (period * repeats).clamp(1., 8.),
    }
}

pub(crate) fn reverb(decay_seconds: f32) -> SendFx {
    SendFx {
        kind: FxKind::Space,
        beats: 1.,
        feedback: 0.35,
        decay_seconds,
        tail_seconds: decay_seconds * 1.5,
    }
}

/// Sounding tempos within 8% at the same metre. The echo keeps the master
/// beat, which passes to the incoming deck at the handoff, so a half/double
/// pairing would change the repeat interval in the middle of the tail.
fn locked(ctx: &PlanContext<'_>, plan: &MixPlan) -> bool {
    let (ga, gb) = (Grid(ctx.outgoing), Grid(ctx.incoming));
    let a = ga.bpm(ga.beat(plan.t_out_a)) * plan.outgoing_offset.rate;
    let b = gb.bpm(gb.beat(plan.t_in_b)) * ctx.offset_b.rate;
    (a / b - 1.).abs() <= 0.08
}

fn outgoing_bpm(ctx: &PlanContext<'_>, plan: &MixPlan) -> f32 {
    let ga = Grid(ctx.outgoing);
    ga.bpm(ga.beat(plan.t_out_a)) * plan.outgoing_offset.rate
}

/// Whether the outgoing voice sounds while its send rises to the peak, the
/// moment the echo captures; a plan that only sends the incoming deck reads
/// the outgoing last moment.
fn sung_into_send(ctx: &PlanContext<'_>, plan: &MixPlan) -> bool {
    let nodes = &plan.lanes.fx_send_a.nodes;
    let Some(peak) = nodes
        .iter()
        .enumerate()
        .max_by(|x, y| x.1.1.total_cmp(&y.1.1))
        .filter(|(_, p)| p.1 > 0.)
        .map(|(i, _)| i)
    else {
        return crate::continuity::voice_active(ctx.outgoing, plan.t_out_a - 0.25) > 0.35;
    };
    let rise = nodes[..peak]
        .iter()
        .rposition(|p| p.1 <= 0.)
        .map_or(0., |i| nodes[i].0);
    let end = nodes[peak].0;
    (0..=8).any(|i| {
        let bar = rise + (end - rise) * i as f32 / 8.;
        crate::continuity::voice_active(ctx.outgoing, plan.outgoing_source.sample(bar)) > 0.35
    })
}

/// Choose the effect for a plan that already sends the outgoing deck.
/// A sung tail stays intelligible through a low-cut echo; an instrumental
/// tail sits darker under the incoming highs.
pub(crate) fn arrange(ctx: &PlanContext<'_>, plan: &mut MixPlan) {
    // An effect tail is still a tonal overlap, including after the dry A
    // fader has closed. Do not let an already selected effect bypass harmony.
    let tail = plan.send_fx.map_or(4., |fx| fx.tail_seconds);
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let ra = ((plan.t_out_a - 2.).max(plan.t_in_a), plan.t_out_a);
    let rb = (
        plan.t_in_b,
        (plan.t_end_b + tail * plan.incoming_offset_end.rate).min(b.duration_sec),
    );
    let (key, shift, reliable) = crate::score::overlap_key_match(
        a,
        b,
        ra,
        rb,
        plan.outgoing_offset.pitch_semitones,
        plan.incoming_offset_end.pitch_semitones,
    )
    .unwrap_or_else(|| {
        crate::score::window_key_match(
            a,
            b,
            plan.t_out_a,
            plan.t_in_b,
            plan.outgoing_offset.pitch_semitones,
            plan.incoming_offset_end.pitch_semitones,
        )
    });
    if !(reliable && key >= 0.85 && shift == 0.)
        && !crate::musical::percussion_only(a, ra.0, ra.1)
        && !crate::musical::percussion_only(b, rb.0, rb.1)
    {
        plan.send_fx = None;
        plan.lanes.fx_send_a = Polyline::constant(0.);
        plan.lanes.fx_send_b = Polyline::constant(0.);
        return;
    }
    if plan.send_fx.is_some()
        || !plan
            .lanes
            .fx_send_a
            .nodes
            .iter()
            .chain(&plan.lanes.fx_send_b.nodes)
            .any(|(_, v)| *v > 0.)
    {
        return;
    }
    plan.send_fx = Some(if locked(ctx, plan) {
        let sung = sung_into_send(ctx, plan);
        echo(
            if sung {
                FxKind::LowCutEcho
            } else {
                FxKind::DubEcho
            },
            0.75,
            0.45,
            outgoing_bpm(ctx, plan),
        )
    } else {
        reverb(2.5)
    });
}

/// A boundary cut can let the outgoing track's last beat ring out under the
/// incoming one instead of stopping dead. Returns false, leaving the cut dry,
/// when a tail would blunt an incoming drop or clash with incoming material.
/// `harmonic` is the measured key compatibility at the boundary.
pub(crate) fn cut_tail(ctx: &PlanContext<'_>, plan: &mut MixPlan, harmonic: bool) -> bool {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let gb = Grid(b);
    // A drop landing is the point of the cut; keep its first kick clean.
    if crate::drops::peaks(b)
        .iter()
        .any(|p| (p.0 - plan.t_in_b).abs() < 0.08)
    {
        return false;
    }
    let entry_bar = gb
        .sec(gb.beat(plan.t_in_b) + gb.meter())
        .min(b.duration_sec);
    let incoming_voice = (0..8)
        .map(|i| {
            crate::continuity::voice_active(
                b,
                plan.t_in_b + (entry_bar - plan.t_in_b) * (i as f32 + 0.5) / 8.,
            )
        })
        .fold(0., f32::max);
    let outgoing_voice = crate::continuity::voice_active(a, plan.t_out_a - 0.25);
    let fx = if harmonic && outgoing_voice > 0.35 && incoming_voice < 0.35 && locked(ctx, plan) {
        // Throw the last sung word: repeats land on the shared beat.
        echo(FxKind::LowCutEcho, 1., 0.4, outgoing_bpm(ctx, plan))
    } else if incoming_voice < 0.35
        && (harmonic || crate::musical::percussion_only(b, plan.t_in_b, entry_bar))
        && feature(a, plan.t_out_a - 0.01).is_some_and(|f| f.rms > 0.001)
    {
        reverb(2.5)
    } else {
        return false;
    };
    // The cut sits at `start`, the incoming launch. Send only its last beat.
    let n = plan.incoming_start_bar;
    let beat = 1. / Grid(a).meter();
    if n < beat {
        return false;
    }
    let peak = if fx.kind == FxKind::Space { 0.35 } else { 0.3 };
    plan.lanes.fx_send_a = Polyline {
        nodes: vec![(0., 0.), (n - beat, 0.), (n - beat * 0.25, peak), (n, 0.)],
    };
    plan.send_fx = Some(fx);
    plan.stages = vec![
        mixless_protocol::MixStage {
            start_bar: 0.,
            end_bar: n,
            label: "Outgoing plays to the phrase end".into(),
        },
        mixless_protocol::MixStage {
            start_bar: n,
            end_bar: plan.clock.nodes.last().map_or(n, |p| p.0).max(n + 0.01),
            label: if fx.kind == FxKind::Space {
                "Cut with reverb tail"
            } else {
                "Cut with echo throw"
            }
            .into(),
        },
    ];
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Planner, PlannerOptions};
    use mixless_protocol::{SectionLabel as S, TransitionMode};

    fn cut(a: &mixless_protocol::TrackAnalysis, b: &mixless_protocol::TrackAnalysis) -> MixPlan {
        Planner::with_options(PlannerOptions::default()).plan_pair(
            a,
            b,
            &[],
            &[],
            Default::default(),
            Default::default(),
        )
    }

    #[test]
    fn an_existing_tail_cannot_bypass_the_sounding_key_check() {
        let a = crate::tests::track(1, 120., "10A", S::Outro, 64, 0.9, 0.2);
        let mut b = crate::tests::track(2, 120., "10B", S::Intro, 64, 0.9, 0.2);
        let mut p = cut(&a, &b);
        p.send_fx = Some(reverb(2.5));
        p.lanes.fx_send_a = Polyline::constant(0.3);
        b.camelot = Some("7B".into());
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        let mut shifted = p.clone();
        shifted.incoming_offset_end.pitch_semitones = -3.;
        arrange(&ctx, &mut shifted);
        assert!(shifted.send_fx.is_some());
        arrange(&ctx, &mut p);
        assert!(p.send_fx.is_none());
        assert_eq!(p.lanes.fx_send_a.sample(0.), 0.);
        assert_eq!(p.lanes.fx_send_b.sample(0.), 0.);
    }

    #[test]
    fn a_tempo_gap_cut_into_a_compatible_intro_rings_out_on_reverb_only() {
        let a = crate::tests::track(1, 128., "8A", S::Outro, 64, 0.9, 0.2);
        let b = crate::tests::track(2, 105., "8A", S::Intro, 64, 0.8, 0.2);
        let p = cut(&a, &b);
        assert_eq!(p.transition_mode, Some(TransitionMode::PhraseBridge));
        let fx = p.send_fx.expect("compatible intro takes a tail");
        assert_eq!(fx.kind, FxKind::Space, "no tempo-bound echo across a gap");
        assert!(fx.valid());
        let n = p.incoming_start_bar;
        assert_eq!(p.summary.as_ref().unwrap().length_bars, 1);
        // Only the last beat feeds the tail; the dry cut is unchanged.
        assert_eq!(p.lanes.fx_send_a.sample(n - 0.3), 0.);
        assert!(p.lanes.fx_send_a.sample(n - 0.05) > 0.2);
        assert_eq!(p.lanes.fx_send_a.sample(n), 0.);
        assert!(p.lanes.gain_a.sample(n) <= -90.);
        assert!((p.lanes.xfader.sample(n) - 1.).abs() < 1e-3);
        assert!((p.lanes.rate_a.sample(n) - 1.).abs() < 1e-3);
        assert!((p.lanes.rate_b.sample(n) - 1.).abs() < 1e-3);
    }

    #[test]
    fn a_cut_onto_an_incoming_drop_stays_dry() {
        let a = crate::tests::track(1, 128., "8A", S::Outro, 64, 0.9, 0.2);
        let mut b = crate::tests::track(2, 105., "8A", S::Drop, 64, 0.9, 0.2);
        let bar = 240. / 105.;
        b.sections = vec![
            mixless_protocol::Section {
                start_sec: 0.,
                end_sec: 32. * bar,
                label: S::Drop,
            },
            mixless_protocol::Section {
                start_sec: 32. * bar,
                end_sec: b.duration_sec,
                label: S::Outro,
            },
        ];
        let mut plan = cut(
            &a,
            &crate::tests::track(2, 105., "8A", S::Intro, 64, 0.8, 0.2),
        );
        plan.send_fx = None;
        plan.t_in_b = 0.;
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        assert!(!cut_tail(&ctx, &mut plan, true));
        assert!(plan.send_fx.is_none());
    }

    #[test]
    fn echo_is_kept_to_a_shared_tempo() {
        let mut a = crate::tests::track(1, 128., "8A", S::Outro, 64, 0.9, 0.2);
        for bar in &mut a.bars {
            bar.vocal_presence = 0.9;
        }
        let b = crate::tests::track(2, 105., "8A", S::Intro, 64, 0.8, 0.2);
        let mut plan = cut(
            &crate::tests::track(1, 128., "8A", S::Outro, 64, 0.9, 0.2),
            &b,
        );
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        plan.send_fx = None;
        if cut_tail(&ctx, &mut plan, true) {
            assert_ne!(plan.send_fx.unwrap().kind, FxKind::LowCutEcho);
        }
        let same = crate::tests::track(2, 128., "8A", S::Intro, 64, 0.8, 0.2);
        let ctx = PlanContext {
            incoming: &same,
            ..ctx
        };
        plan.send_fx = None;
        assert!(cut_tail(&ctx, &mut plan, true));
        assert_eq!(
            plan.send_fx.unwrap().kind,
            FxKind::LowCutEcho,
            "sung tail is thrown"
        );
    }

    #[test]
    fn echo_tails_stop_ringing_within_bounds() {
        let fx = echo(FxKind::LowCutEcho, 0.75, 0.45, 128.);
        assert!(fx.valid());
        assert!(fx.tail_seconds >= 1. && fx.tail_seconds <= 8.);
        assert!(reverb(2.5).valid());
    }
}
