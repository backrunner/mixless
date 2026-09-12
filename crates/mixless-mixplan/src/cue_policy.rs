//! Saved AUTO pads are candidate anchors; explicit IN/OUT pads are alternative
//! windows. They do not force one transition to span every marker in a track.
use mixless_protocol::{Cue, CueKind, MixRegionKind, TrackAnalysis};

pub fn cue_choices(cues: &[Cue], outgoing: bool) -> Vec<Vec<Cue>> {
    let role = if outgoing { CueKind::Out } else { CueKind::In };
    let base: Vec<_> = cues
        .iter()
        .filter(|c| !c.user_set || c.kind == CueKind::Hot)
        .cloned()
        .collect();
    let choices: Vec<_> = cues
        .iter()
        .filter(|c| c.user_set && c.kind == role)
        .map(|anchor| {
            let mut choice = base.clone();
            choice.push(anchor.clone());
            choice
        })
        .collect();
    if choices.is_empty() {
        vec![base]
    } else {
        choices
    }
}

/// Nearby analyzed windows decide the role where evidence is available.
/// Otherwise prefer the first half for entry and the second half for exit.
pub fn inferred_cue_kind(track: &TrackAnalysis, frame: u64) -> CueKind {
    let sec = frame as f32 / track.sample_rate.max(1) as f32;
    let region = track
        .mix_regions
        .iter()
        .filter(|r| sec >= r.start_sec && sec <= r.end_sec)
        .max_by(|a, b| {
            let merit = |r: &mixless_protocol::MixRegion| {
                r.confidence / (1. + (r.anchor_sec - sec).abs() / 8.)
            };
            merit(a).total_cmp(&merit(b))
        });
    match region.map(|r| r.kind) {
        Some(MixRegionKind::In) => CueKind::In,
        Some(MixRegionKind::Out) => CueKind::Out,
        None if sec < track.duration_sec * 0.5 => CueKind::In,
        None => CueKind::Out,
    }
}

pub(crate) fn auto_anchors(track: &TrackAnalysis, cues: &[Cue], outgoing: bool) -> Vec<f32> {
    let role = if outgoing { CueKind::Out } else { CueKind::In };
    let grid = crate::grid::Grid(track);
    cues.iter()
        .filter(|c| {
            c.user_set && c.kind == CueKind::Hot && inferred_cue_kind(track, c.frame) == role
        })
        .map(|c| {
            let sec = c.frame as f32 / track.sample_rate.max(1) as f32;
            if outgoing {
                grid.ceil_bar(sec)
            } else {
                grid.floor_bar(sec)
            }
        })
        .collect()
}

pub(crate) fn manual_quality(track: &TrackAnalysis, cues: &[Cue], sec: f32, outgoing: bool) -> f32 {
    let g = crate::grid::Grid(track);
    let role = if outgoing { CueKind::Out } else { CueKind::In };
    if cues.iter().any(|c| {
        if !c.user_set || c.kind != CueKind::Hot || inferred_cue_kind(track, c.frame) != role {
            return false;
        }
        let cue_sec = c.frame as f32 / track.sample_rate.max(1) as f32;
        let beat = if outgoing {
            g.ceil_bar(cue_sec)
        } else {
            g.floor_bar(cue_sec)
        };
        (g.sec(beat) - sec).abs() < 0.08
    }) {
        0.72
    } else {
        0.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiple_entry_and_exit_markers_are_choices_not_one_enormous_range() {
        let cues: Vec<_> = [
            CueKind::In,
            CueKind::In,
            CueKind::Out,
            CueKind::Out,
            CueKind::Hot,
        ]
        .into_iter()
        .enumerate()
        .map(|(i, kind)| Cue {
            index: i as u8,
            frame: i as u64 * 48000,
            kind,
            user_set: true,
        })
        .collect();
        for outgoing in [false, true] {
            let choices = cue_choices(&cues, outgoing);
            assert_eq!(choices.len(), 2);
            for choice in choices {
                assert_eq!(choice.len(), 2);
                assert_eq!(choice.iter().filter(|c| c.kind != CueKind::Hot).count(), 1);
                assert_eq!(
                    choice.last().unwrap().kind,
                    if outgoing { CueKind::Out } else { CueKind::In }
                );
            }
        }
    }
}
