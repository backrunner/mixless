//! A playlist replacement is a musical handoff, not a fresh playlist start.
use super::*;

#[test]
fn later_loud_bars_do_not_hide_a_fading_first_bar_in_normal_or_fallback_entries() {
    for tempo in [90., 120., 175.] {
        for gain in [0.25, 1., 2.] {
            let mut a = track(1, tempo, "8A", S::Outro, 96, 0.8, 0.2 * gain);
            for bar in &mut a.bars[88..] {
                bar.rms = 0.05 * gain;
            }
            let mut b = track(2, tempo * 0.75, "3B", S::Intro, 64, 0.8, 0.2 * gain);
            b.bars[0].rms = 0.015 * gain;
            let mut ctx = PlanContext {
                outgoing: &a,
                incoming: &b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            };
            let later_phrase = b.bars[8].start_sec;
            for plan in [
                Planner::new().plan_next(&ctx),
                crate::short_handoff(&ctx, 20.),
            ] {
                assert!(plan.failure_reason.is_none(), "{:?}", plan.failure_reason);
                assert!(
                    plan.t_in_b >= later_phrase - 0.001,
                    "fade-in entry {}",
                    plan.t_in_b
                );
            }
            // Explicit user intent is not replaced by the automatic preference.
            let manual = [Cue {
                index: 0,
                frame: 0,
                kind: CueKind::In,
                user_set: true,
            }];
            ctx.cues_in = &manual;
            for plan in [
                Planner::new().plan_next(&ctx),
                crate::short_handoff(&ctx, 20.),
            ] {
                assert!(plan.failure_reason.is_none(), "{:?}", plan.failure_reason);
                close(plan.t_in_b, 0.);
            }
        }
    }
}

#[test]
fn established_or_consistently_quiet_intro_can_still_start_at_the_beginning() {
    for level in [0.03, 0.2] {
        let a = track(1, 120., "8A", S::Outro, 64, 0.8, level);
        let b = track(2, 90., "3B", S::Intro, 64, 0.8, level);
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        for plan in [
            Planner::new().plan_next(&ctx),
            crate::short_handoff(&ctx, 20.),
        ] {
            assert!(plan.failure_reason.is_none(), "{:?}", plan.failure_reason);
            close(plan.t_in_b, 0.);
        }
    }
}
