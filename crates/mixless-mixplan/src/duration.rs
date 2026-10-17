//! Enumerate musical spans, not a fixed crossfade duration.
use crate::{PlanContext, PlannerOptions, grid::Grid};
use mixless_protocol::{MixRegionKind as K, TransitionMode as M};

pub(super) fn candidates(
    ctx: &PlanContext<'_>,
    opts: &PlannerOptions,
    out: f32,
    input: f32,
    mode: M,
) -> Vec<u16> {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let (ga, gb) = (Grid(a), Grid(b));
    let max = ((out - ga.ceil_bar(opts.earliest_outgoing_sec)) / ga.meter())
        .floor()
        .clamp(0., 128.) as u16;
    if mode == M::PhraseBridge {
        return [0, 2, 4, 8, 16, 24, 1]
            .into_iter()
            .filter(|n| *n <= max)
            .collect();
    }
    let ratio = ga.bpm(out) * ctx.offset_a.rate / (gb.bpm(input) * ctx.offset_b.rate);
    let factor = crate::grid::tempo_factor(ratio);
    let mut spans = vec![4., 8., 16., 24., 32., 48., 64.];
    let mut recovery_spans = vec![];
    // Keep actual outgoing arrangement starts as well as incoming landings.
    // Otherwise dense incoming subdivisions can evict the only span whose
    // beginning lets A finish its sung phrase over B's instrumental bed.
    let mut departure_spans: Vec<_> = a
        .sections
        .iter()
        .map(|s| (out - ga.ceil_bar(s.start_sec)) / ga.meter())
        .filter(|n| *n >= 4. && *n <= max as f32 && (*n - n.round()).abs() < 0.02)
        .map(|n| n.round() as u16)
        .collect();
    departure_spans.sort_unstable();
    departure_spans.dedup();
    // Keep the lengths that resolve exactly onto B's peak. Sampling the
    // sorted lengths can discard, for example, a 14-bar build in favour of
    // 8/16 bars; neither substitute can land B's drop on A's phrase ending.
    let landing_spans: Vec<_> = crate::drops::peaks(b)
        .iter()
        .map(|p| (gb.beat(p.0) - input) / (ga.meter() * factor))
        .filter(|n| *n >= 4. && *n <= max as f32 && (*n - n.round()).abs() < 0.02)
        .map(|n| n.round() as u16)
        .collect();
    spans.extend(landing_spans.iter().map(|n| *n as f32));
    for (start, end) in crate::recovery::windows(a) {
        if ga.sec(out) <= end + 0.05 {
            let length = (out - ga.ceil_bar(start)) / ga.meter();
            spans.push(length);
            if length >= 4. && length <= max as f32 && (length - length.round()).abs() < 0.02 {
                recovery_spans.push(length.round() as u16);
            }
        }
    }
    spans.extend(
        a.phrase_boundaries
            .iter()
            .filter(|p| p.confidence >= 0.5)
            .map(|p| (out - ga.beat(p.time_sec)) / ga.meter()),
    );
    spans.extend(
        b.phrase_boundaries
            .iter()
            .filter(|p| p.confidence >= 0.5)
            .map(|p| (gb.beat(p.time_sec) - input) / (ga.meter() * factor)),
    );
    spans.extend(
        a.mix_regions
            .iter()
            .filter(|r| r.kind == K::Out && (ga.sec(out) - r.anchor_sec).abs() < 0.15)
            .map(|r| (out - ga.beat(r.start_sec)) / ga.meter()),
    );
    spans.extend(
        b.mix_regions
            .iter()
            .filter(|r| r.kind == K::In && (gb.sec(input) - r.anchor_sec).abs() < 0.15)
            .map(|r| (gb.beat(r.end_sec) - input) / (ga.meter() * factor)),
    );
    // Hand-annotated/legacy tracks still expose their actual section lengths.
    spans.extend(
        a.sections
            .iter()
            .map(|s| (out - ga.ceil_bar(s.start_sec)) / ga.meter()),
    );
    spans.extend(
        b.sections
            .iter()
            .map(|s| (gb.floor_bar(s.end_sec) - input) / (ga.meter() * factor)),
    );
    let mut spans: Vec<_> = spans
        .into_iter()
        .filter(|n| {
            n.is_finite() && *n >= 4. && *n <= max as f32 + 0.01 && (*n - n.round()).abs() < 0.02
        })
        .map(|n| n.round() as u16)
        .filter(|n| mode != M::LoopRoll || *n <= 16)
        .collect();
    spans.sort_unstable();
    spans.dedup();
    // Bound candidate work while retaining short, middle and long phrases.
    if spans.len() > 12 {
        let mut selected = recovery_spans;
        selected.retain(|n| spans.contains(n));
        selected.sort_unstable();
        selected.dedup();
        selected.truncate(8);
        for i in 0..12 {
            let n = spans[i * (spans.len() - 1) / 11];
            if selected.len() < 12 && !selected.contains(&n) {
                selected.push(n);
            }
        }
        // Extend the existing search rather than displacing previously safe
        // short/middle/long candidates. At most eight peak landings add work.
        for n in landing_spans
            .into_iter()
            .filter(|n| spans.contains(n))
            .take(8)
            // Retain nearby phrase endings as well. A later entrance reduces
            // the available long spans, changing uniform sample indices; it
            // must not evict a still-feasible short phrase from the search.
            .chain(spans.iter().copied().take(8))
            .chain(
                departure_spans
                    .into_iter()
                    .filter(|n| spans.contains(n))
                    .take(8),
            )
        {
            if !selected.contains(&n) {
                selected.push(n);
            }
        }
        selected.sort_unstable();
        spans = selected;
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{PhraseBoundary, Section, SectionLabel as S};

    #[test]
    fn bounded_search_keeps_irregular_builds_that_land_on_the_incoming_peak() {
        for bpm in [90., 142., 175.] {
            for landing in [6, 14, 23] {
                let a = crate::tests::track(1, bpm, "8A", S::Outro, 128, 0.9, 0.2);
                let mut b = crate::tests::track(2, bpm, "8A", S::Intro, 128, 0.9, 0.2);
                let bar = 240. / bpm;
                b.sections = vec![
                    Section {
                        start_sec: 0.,
                        end_sec: landing as f32 * bar,
                        label: S::BuildUp,
                    },
                    Section {
                        start_sec: landing as f32 * bar,
                        end_sec: 96. * bar,
                        label: S::Drop,
                    },
                    Section {
                        start_sec: 96. * bar,
                        end_sec: 128. * bar,
                        label: S::Outro,
                    },
                ];
                // Enough alternatives to trigger the bounded-search sampling.
                b.phrase_boundaries = (1..120)
                    .map(|i| PhraseBoundary {
                        time_sec: i as f32 * bar,
                        confidence: 0.8,
                        novelty: 0.3,
                    })
                    .collect();
                let ctx = PlanContext {
                    outgoing: &a,
                    incoming: &b,
                    cues_out: &[],
                    cues_in: &[],
                    offset_a: Default::default(),
                    offset_b: Default::default(),
                };
                let options = PlannerOptions::default();
                let spans = candidates(&ctx, &options, 96. * 4., 0., M::BeatBlend);
                assert!(spans.contains(&landing), "{bpm}: {spans:?}");
                assert!(spans.len() <= 36);
                assert!((4..=11).all(|n| spans.contains(&n)));
                let limited = PlannerOptions {
                    earliest_outgoing_sec: 94. * bar,
                    ..options
                };
                assert!(candidates(&ctx, &limited, 96. * 4., 0., M::BeatBlend).is_empty());
                assert!(
                    candidates(&ctx, &PlannerOptions::default(), 96. * 4., 0., M::LoopRoll)
                        .iter()
                        .all(|n| *n <= 16)
                );
            }
        }
    }
}

#[cfg(test)]
#[test]
fn dense_incoming_phrases_do_not_evict_an_outgoing_arrangement_start() {
    use mixless_protocol::{PhraseBoundary, Section, SectionLabel as S};
    for bpm in [90., 156., 180.] {
        let bar = 240. / bpm;
        let mut a = crate::tests::track(1, bpm, "8A", S::Outro, 128, 0.8, 0.2);
        a.sections = vec![
            Section {
                start_sec: 0.,
                end_sec: 49. * bar,
                label: S::Chorus,
            },
            Section {
                start_sec: 49. * bar,
                end_sec: 128. * bar,
                label: S::Outro,
            },
        ];
        let mut b = a.clone();
        b.sections.clear();
        b.phrase_boundaries = (1..100)
            .map(|i| PhraseBoundary {
                time_sec: i as f32 * bar,
                confidence: 0.8,
                novelty: 0.2,
            })
            .collect();
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        let spans = candidates(&ctx, &PlannerOptions::default(), 96. * 4., 0., M::BeatBlend);
        assert!(
            spans.contains(&47),
            "outgoing section start lost: {spans:?}"
        );
        assert!(spans.len() <= 36);
    }
}
