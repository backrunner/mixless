//! Resolve empty bounded searches with every short whole-bar span, using the
//! same eligibility, arrangement, vocal and foreground gates as normal pairs.
//! The normal pool keeps precedence; more windows are not inherently better.
use crate::{PlanContext, PlannerOptions};
use mixless_protocol::{MixPlan, TransitionMode};
type Scored = Option<(f32, MixPlan)>;

pub(super) fn search(
    ctx: &PlanContext<'_>,
    opts: &PlannerOptions,
    outs: &[f32],
    ins: &[f32],
) -> (Scored, Vec<Scored>) {
    let evidence = crate::evidence::Prepared::new(ctx, opts.stem_playback);
    let run = |index: usize| {
        evidence.run(|| {
            let mut best: Option<((usize, usize, usize, usize), f32, MixPlan)> = None;
            for (oi, &out) in outs.iter().enumerate() {
                for (mi, mode) in [TransitionMode::BeatBlend, TransitionMode::LoopRoll]
                    .into_iter()
                    .enumerate()
                {
                    for length in crate::duration::supplemental(ctx, opts, out, ins[index], mode) {
                        let Some(plan) = super::pair(
                            ctx,
                            opts,
                            out,
                            ins[index],
                            mode,
                            length,
                            best.as_ref().map_or(-1., |p| p.1),
                        ) else {
                            continue;
                        };
                        let merit = crate::experience::merit(&plan, ctx.outgoing, ctx.incoming);
                        if best.as_ref().is_none_or(|old| merit > old.1) {
                            best = Some(((oi, index, mi, length as usize), merit, plan));
                        }
                    }
                }
            }
            best
        })
    };
    let results: Vec<_> = if cfg!(feature = "trace") {
        (0..ins.len()).map(run).collect()
    } else {
        crate::order::parallel::map(ins.len(), run)
    };
    let mut best: Option<&((usize, usize, usize, usize), f32, MixPlan)> = None;
    for p in results.iter().flatten() {
        if best.is_none_or(|old| p.1 > old.1 || p.1 == old.1 && p.0 < old.0) {
            best = Some(p);
        }
    }
    let best = best.map(|p| (p.1, p.2.clone()));
    (
        best,
        results
            .into_iter()
            .map(|p| p.map(|(_, score, plan)| (score, plan)))
            .collect(),
    )
}
