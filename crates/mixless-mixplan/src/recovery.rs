//! A drop's recovery is a mixing interval, not a cue where a short fade must end.
use crate::grid::Grid;
use mixless_protocol::{SectionLabel as S, TrackAnalysis};

pub(crate) fn windows(t: &TrackAnalysis) -> Vec<(f32, f32)> {
    let mut result = vec![];
    for (_, drop_end) in crate::drops::peaks(t) {
        if mixless_protocol::drop_suspension(t, drop_end).is_some() {
            continue;
        }
        let Some(first) = t.sections.iter().position(|s| {
            (s.start_sec - drop_end).abs() < 0.08
                && matches!(s.label, S::Break | S::Breakdown | S::Outro)
        }) else {
            continue;
        };
        let mut end = drop_end;
        for section in &t.sections[first..] {
            if !matches!(section.label, S::Break | S::Breakdown | S::Outro) {
                break;
            }
            end = section.end_sec;
        }
        // The interval begins at the measured change in arrangement. There is
        // no mandatory bar before/after a drop and no fixed vocal-tail padding.
        let start = drop_end;
        if end > start {
            result.push((start, end));
        }
    }
    result
}

pub(crate) fn contains(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    windows(t)
        .iter()
        .any(|&(a, b)| start >= a - 0.05 && end <= b + 0.05 && end > start)
}

pub(crate) fn quality(t: &TrackAnalysis, sec: f32) -> f32 {
    let g = Grid(t);
    if (g.sec(g.floor_bar(sec)) - sec).abs() < 0.08
        && windows(t)
            .iter()
            .any(|&(a, b)| sec >= a - 0.05 && sec <= b + 0.05)
    {
        0.65
    } else {
        0.
    }
}
