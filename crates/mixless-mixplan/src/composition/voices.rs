//! Search independent vocal rests without losing the sequential alternative.
use super::Fade;

pub(super) fn pairs(
    voices: &[Vec<Fade>; 2],
    tonal: f32,
    handoff: f32,
    harmonic: bool,
    manual: bool,
) -> Vec<(Fade, Fade)> {
    let mut selected: Vec<(Fade, Fade)> = Vec::new();
    // Preserve the ordinary sequential fit, then consider naturally
    // interleaved singers. Actual simultaneous voice power is checked by cost.
    for interleaved in [false, true] {
        if interleaved && !harmonic {
            continue;
        }
        let mut pairs: Vec<_> = voices[0]
            .iter()
            .copied()
            .filter_map(|out| {
                // Cross-key parts follow the fitted melody release, not this seed.
                // Without a measured rest the incoming singer is held for the
                // phrasing fit, which may land it whole on the arrival downbeat.
                if !harmonic && !manual {
                    let held = Fade {
                        start: f32::INFINITY,
                        end: f32::INFINITY,
                    };
                    return Some((out, voices[1].first().copied().unwrap_or(held)));
                }
                let threshold = if interleaved {
                    0.
                } else if harmonic {
                    out.end
                } else {
                    out.end.max(tonal).max(handoff)
                };
                let first = voices[1].partition_point(|f| f.start < threshold - 0.001);
                let closest = voices[1].partition_point(|f| f.start < tonal).max(first);
                [first, closest.saturating_sub(1).max(first), closest]
                    .into_iter()
                    .filter_map(|i| voices[1].get(i).copied())
                    .min_by(|a, b| (a.start - tonal).abs().total_cmp(&(b.start - tonal).abs()))
                    .map(|input| (out, input))
            })
            .collect();
        let distance = |p: &(Fade, Fade)| (p.0.start - tonal).abs() + (p.1.start - tonal).abs();
        pairs.sort_by(|a, b| distance(a).total_cmp(&distance(b)));
        for pair in pairs.iter().take(4) {
            if !selected
                .iter()
                .any(|s| s.0.start == pair.0.start && s.1.start == pair.1.start)
            {
                selected.push(*pair);
            }
        }
    }
    selected
}
