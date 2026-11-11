//! Independent tonal/kit seeds retain their serial coordinate descent.
//! Keep all five kit anchors in each tonal job to reuse melodic windows and
//! amortize evidence setup. Ordered flattening keeps the first exact schedule.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn candidates(
    p: &MixPlan,
    ctx: &PlanContext<'_>,
    tonality: Tonality,
    frames: &[Frame],
    reference: [f32; 2],
    n: f32,
    manual: bool,
    voices: &[Vec<Fade>; 2],
    protected: f32,
    handoff: f32,
    participation: Option<&[participation::Sample]>,
    duration_reward: &(impl Fn(f32) -> f32 + Sync),
) -> Vec<evaluation::Candidate> {
    generate(
        p,
        ctx,
        tonality,
        frames,
        reference,
        n,
        manual,
        voices,
        protected,
        handoff,
        participation,
        duration_reward,
        !cfg!(feature = "trace"),
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn generate(
    p: &MixPlan,
    ctx: &PlanContext<'_>,
    tonality: Tonality,
    frames: &[Frame],
    reference: [f32; 2],
    n: f32,
    manual: bool,
    voices: &[Vec<Fade>; 2],
    protected: f32,
    handoff: f32,
    participation: Option<&[participation::Sample]>,
    duration_reward: &(impl Fn(f32) -> f32 + Sync),
    parallel: bool,
) -> Vec<evaluation::Candidate> {
    let harmonic = tonality == Tonality::Compatible;
    let costs = search::Costs::new(frames, tonality);
    let evidence = crate::evidence::Prepared::new(ctx, true);
    let job = |index: usize| {
        let mut windows = crate::melody::Search::new(ctx, p, n);
        let mut candidates = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut consider = |shape: Shape| {
            let key = search::key(&shape);
            if !seen.insert(key) {
                return;
            }
            // Count the audible overlap, not a reserved playback window.
            if let Some(c) = costs.get(&shape) {
                let base = (c / 100.).min(0.1);
                let upper = participation.as_ref().map_or(n, |samples| {
                    participation::upper(samples, &shape, reference)
                });
                let lower_cost = base + (duration_reward(n) - duration_reward(upper)).max(0.);
                candidates.push(evaluation::Candidate {
                    shape,
                    base,
                    lower_cost,
                });
            }
        };
        if index == 4 {
            for shape in alternatives::shapes(&mut windows, p, ctx, voices, tonality, n) {
                consider(shape);
            }
        } else {
            let tonal = [handoff - 4., handoff, handoff + 4., protected][index].clamp(1., n - 0.5);
            for drums in [
                handoff - 4.,
                handoff - 2.,
                handoff,
                handoff + 2.,
                handoff + 4.,
            ] {
                let drums = drums.clamp(0.5, n - 0.25);
                let mut shape = [[Fade { start: 0., end: n }; 4]; 2];
                for (part, anchor, width) in [(1, drums, 1.), (2, tonal, 2.), (3, handoff, 0.25)] {
                    let end = (anchor + width).min(n);
                    shape[0][part] = Fade { start: anchor, end };
                    shape[1][part] = shape[0][part];
                }
                for (voice_out, voice_in) in voices::pairs(voices, tonal, handoff, harmonic, manual)
                {
                    let mut shape = shape;
                    shape[0][0] = voice_out;
                    shape[1][0] = voice_in;
                    if !harmonic && manual {
                        // Unknown/different keys: every tonal part of A must be out
                        // before any tonal part of B is exposed. Drums can still ride.
                        let gate = voice_out.end.max(tonal).max(handoff);
                        if gate > n - 0.25 || voice_in.start < gate {
                            continue;
                        }
                        for part in [2, 3] {
                            shape[0][part] = Fade {
                                start: (gate - 1.).max(0.25),
                                end: gate,
                            };
                            shape[1][part] = Fade {
                                start: gate,
                                end: (gate + 1.).min(n),
                            };
                        }
                    }
                    // Coordinate descent, with independent widths on the two decks.
                    // Voice fades stay inside measured silent breaths and bass stays
                    // complementary, so neither can be stretched into an unsafe line.
                    for part in [1] {
                        for deck in 0..2 {
                            let mut selected = shape;
                            let mut trials = [shape; 5];
                            for (i, width) in [0.5, 1., 2., 4.].into_iter().enumerate() {
                                let mut variant = shape;
                                if !harmonic && part == 2 && deck == 0 {
                                    variant[deck][part].start =
                                        (variant[deck][part].end - width).max(0.);
                                } else {
                                    variant[deck][part].end =
                                        (variant[deck][part].start + width).min(n);
                                }
                                trials[i + 1] = variant;
                            }
                            let scores = costs.batch(&trials);
                            let mut current = scores[0].unwrap_or(f32::INFINITY);
                            for (variant, score) in trials.into_iter().zip(scores).skip(1) {
                                if let Some(c) = score.filter(|c| *c < current - 1e-4) {
                                    selected = variant;
                                    current = c;
                                }
                            }
                            shape = selected;
                        }
                    }
                    let shapes = if manual {
                        vec![shape]
                    } else {
                        phrasing::shapes(
                            &mut windows,
                            p,
                            ctx,
                            shape,
                            &voices[1],
                            tonality,
                            protected,
                            n,
                        )
                    };
                    for shape in shapes {
                        consider(shape);
                    }
                }
            }
        }
        candidates
    };
    let groups = if !parallel {
        (0..5).map(job).collect()
    } else {
        crate::order::parallel::map(5, |i| evidence.run(|| job(i)))
    };
    let mut seen = std::collections::HashSet::new();
    groups
        .into_iter()
        .flatten()
        .filter(|c| seen.insert(search::key(&c.shape)))
        .collect()
}
