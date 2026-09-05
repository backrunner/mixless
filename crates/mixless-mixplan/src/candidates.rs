use crate::{constraints::user_range, grid::Grid};
use mixless_protocol::{Cue, SectionLabel as S, TrackAnalysis};

pub(crate) fn candidates(track: &TrackAnalysis, cues: &[Cue], outgoing: bool) -> Vec<f32> {
    let grid = Grid(track);
    let mut result = Vec::new();
    if let Some((start, end)) = user_range(cues, track.sample_rate) {
        result.push(if outgoing {
            grid.ceil_bar(end)
        } else {
            grid.floor_bar(start)
        });
    }
    let mut sections: Vec<_> = track.sections.iter().collect();
    if outgoing {
        sections.reverse();
    }
    for section in sections {
        if outgoing
            && matches!(
                section.label,
                S::Drop | S::Chorus | S::Outro | S::Break | S::BuildUp
            )
        {
            result.push(grid.floor_bar(section.end_sec));
        } else if !outgoing
            && matches!(
                section.label,
                S::Intro | S::Drop | S::BuildUp | S::Break | S::Breakdown
            )
        {
            if section.label == S::Intro {
                result.push(grid.floor_bar((section.start_sec + section.end_sec) * 0.5));
            }
            result.push(grid.ceil_bar(section.start_sec));
        }
    }
    if outgoing {
        result.push(grid.floor_bar(track.duration_sec));
    } else {
        result.push(grid.ceil_bar(0.0));
    }
    result.retain(|b| b.is_finite() && *b >= 0.0 && grid.sec(*b) <= track.duration_sec + 0.001);
    let mut unique = Vec::new();
    for b in result {
        if !unique.iter().any(|v: &f32| (*v - b).abs() < 0.01) {
            unique.push(b);
        }
    }
    unique.truncate(8);
    unique
}
