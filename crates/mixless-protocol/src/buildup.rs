//! Shared measured build evidence for classification and transition eligibility.
//! A bass retreat followed by a loud section alone is a break, not a buildup.
use crate::BarFeature;

pub fn has_buildup(bars: &[BarFeature], start: f32, end: f32) -> bool {
    let first = bars.partition_point(|b| b.end_sec <= start + 0.01);
    let last = bars.partition_point(|b| b.start_sec < end - 0.01);
    let Some(part) = bars.get(first..last) else {
        return false;
    };
    // A final breath belongs to the build's resolution. Only trim at most
    // two trailing silent bars; an internal or extended silence is not a roll.
    let sounding = part
        .iter()
        .rposition(|b| b.rms > 0.001)
        .map_or(0, |i| i + 1);
    if part.len() - sounding > 2 {
        return false;
    }
    let part = &part[..sounding];
    if part.len() < 3 || part.iter().any(|b| b.rms <= 0.001) {
        return false;
    }
    let width = (part.len() / 4).clamp(2, 4).min(part.len() / 2);
    let mean = |slice: &[BarFeature], f: fn(&BarFeature) -> f32| {
        slice.iter().map(f).sum::<f32>() / slice.len().max(1) as f32
    };
    let head = &part[..width];
    // A one-bar breath/fill immediately before the drop can lower the final
    // average; require a sustained rise in the latter half, not a final spike.
    let tail_start = (part.len() / 2).min(part.len() - width - 1);
    let tail = part[tail_start..part.len() - 1]
        .windows(width)
        .max_by(|a, b| mean(a, |b| b.rms).total_cmp(&mean(b, |b| b.rms)))
        .unwrap();
    let level = |b: &BarFeature| b.rms;
    let attacks = |b: &BarFeature| b.onset_density;
    let bright = |b: &BarFeature| b.high_db - b.low_db;
    let consistent = |f: fn(&BarFeature) -> f32| {
        part.windows(2)
            .filter(|w| f(&w[1]) >= f(&w[0]) * 0.95)
            .count()
            * 3
            >= (part.len() - 1) * 2
    };
    let rises = part
        .windows(2)
        .filter(|w| w[1].rms > w[0].rms * 1.05)
        .count();
    let swell = mean(tail, level) > mean(head, level) * 1.3
        && consistent(level)
        && rises >= 2
        && tail.iter().all(|b| b.rms > mean(head, level) * 1.15);
    let roll = mean(tail, attacks) > mean(head, attacks).max(0.1) * 1.5
        && consistent(attacks)
        && (mean(tail, bright) > mean(head, bright) + 2.
            || mean(tail, level) > mean(head, level) * 1.1);
    // A sustained roll may start abruptly after a breakdown and hold its
    // intensity. Compare with the preceding bars, not with the next drop.
    let previous = &bars[first.saturating_sub(3)..first];
    let sustained_roll = previous.len() >= 2
        && part.len() <= 16
        && mean(head, attacks) > mean(previous, attacks).max(0.1) * 1.5
        && (mean(head, level) > mean(previous, level).max(0.001) * 1.25
            || (mean(head, bright) > mean(previous, bright) + 2.
                && mean(tail, bright) > mean(head, bright) + 1.5))
        && mean(tail, attacks) >= mean(head, attacks) * 0.85
        && mean(tail, level) >= mean(head, level) * 0.85;
    // Limited electronic builds can lose bass without rising in RMS or
    // attack count. Require sustained treble growth as well as bass retreat;
    // one returning kick after a quiet pause cannot establish this ramp.
    let bright_tail = part[tail_start..part.len() - 1]
        .windows(width)
        .max_by(|a, b| mean(a, |b| b.high_db).total_cmp(&mean(b, |b| b.high_db)))
        .unwrap();
    let spectral_riser = mean(bright_tail, bright) > mean(head, bright) + 6.
        && mean(bright_tail, |b| b.high_db) > mean(head, |b| b.high_db) + 3.
        && mean(bright_tail, attacks) >= mean(head, attacks).max(1.) * 0.8
        && part
            .windows(2)
            .filter(|w| bright(&w[1]) >= bright(&w[0]) - 1.)
            .count()
            * 3
            >= (part.len() - 1) * 2;
    swell || roll || sustained_roll || spectral_riser
}
