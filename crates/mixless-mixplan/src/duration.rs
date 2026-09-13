//! Enumerate musical spans, not a fixed crossfade duration.
use crate::{grid::Grid, PlanContext, PlannerOptions};
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
    let factor = if (ratio / 2. - 1.).abs() < 0.08 {
        0.5
    } else if (ratio / 0.5 - 1.).abs() < 0.08 {
        2.
    } else {
        1.
    };
    let mut spans = vec![4., 8., 16., 24., 32., 48., 64.];
    let mut recovery_spans = vec![];
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
        selected.sort_unstable();
        spans = selected;
    }
    spans
}
