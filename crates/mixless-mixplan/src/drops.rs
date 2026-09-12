//! Prefer one meaningful peak per play-through; later exits remain fallbacks.
use mixless_protocol::{SectionLabel as S, TrackAnalysis};

/// A section boundary inside one continuous drop is a variation, not an exit.
pub(super) fn peaks(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    let mut result: Vec<(f32, f32)> = Vec::new();
    // A legacy whole-file label supplies no measured drop boundaries.
    if track.sections.len() < 2 {
        return result;
    }
    for s in &track.sections {
        if !matches!(s.label, S::Drop | S::Chorus) {
            continue;
        }
        if let Some(last) = result.last_mut() {
            if s.start_sec <= last.1 + 0.1 {
                last.1 = last.1.max(s.end_sec);
                continue;
            }
        }
        result.push((s.start_sec, s.end_sec));
    }
    result
}

/// Protect the entire first available peak on this pass, then every later peak
/// once entered. Check the *start* of an overlap, not just its final out marker.
pub(super) fn allows(track: &TrackAnalysis, start: f32, end: f32, entry: f32, cut: bool) -> bool {
    let peaks = peaks(track);
    let first = peaks
        .iter()
        .find(|p| p.0 + 0.1 >= entry)
        .or_else(|| peaks.iter().find(|p| p.1 > entry + 0.1));
    if let Some(first) = first {
        if (if cut { end } else { start }) + 0.05 < first.1 {
            return false;
        }
    }
    !peaks.iter().any(|&(a, b)| {
        if cut {
            end > a + 0.05 && end < b - 0.05
        } else {
            start < b - 0.05 && end > a + 0.05
        }
    })
}

pub(super) fn recovery(label: S) -> bool {
    matches!(label, S::Break | S::Breakdown | S::BuildUp | S::Outro)
}

pub(super) fn entry_allowed(track: &TrackAnalysis, entry: f32) -> bool {
    !peaks(track)
        .iter()
        .any(|&(start, end)| entry > start + 0.05 && entry < end - 0.05)
}

pub(super) fn penalty(track: &TrackAnalysis, exit: f32, entry: f32) -> f32 {
    let mut peaks: Vec<(f32, f32)> = Vec::new();
    for section in &track.sections {
        if !matches!(section.label, S::Drop | S::Chorus)
            || section.end_sec <= entry + 1.
            || section.end_sec - section.start_sec < 4.
        {
            continue;
        }
        if let Some(last) = peaks.last_mut() {
            if section.start_sec <= last.1 + 1. {
                last.1 = section.end_sec;
                continue;
            }
        }
        peaks.push((section.start_sec, section.end_sec));
    }
    let Some(first) = peaks.first() else {
        return 0.;
    };
    if exit + 0.1 < first.1 {
        return 0.22;
    }
    let Some(second) = peaks.get(1) else {
        return 0.;
    };
    if exit <= second.0 + 0.1 {
        return 0.;
    }
    let heard = ((exit - second.0) / (second.1 - second.0)).clamp(0., 1.);
    0.16 + heard * 0.16 + peaks.iter().skip(2).filter(|p| exit > p.0).count() as f32 * 0.08
}
