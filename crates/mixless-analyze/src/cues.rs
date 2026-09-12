//! Stable numbered suggestions from measured structure; manual slots take priority.
use mixless_protocol::{Cue, CueKind, MixRegionKind, TrackAnalysis};

pub fn automatic_cues(t: &TrackAnalysis) -> Vec<Cue> {
    let mut candidates: Vec<_> = t
        .mix_regions
        .iter()
        .map(|r| {
            (
                r.anchor_sec,
                r.confidence,
                if r.kind == MixRegionKind::In {
                    CueKind::In
                } else {
                    CueKind::Out
                },
            )
        })
        .collect();
    candidates.extend(
        t.phrase_boundaries
            .iter()
            .filter(|p| p.novelty >= 0.15 && p.confidence >= 0.6)
            .map(|p| (p.time_sec, p.confidence, CueKind::Hot)),
    );
    if candidates.is_empty() {
        return vec![];
    }
    let first = candidates
        .iter()
        .filter(|c| c.2 == CueKind::In)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .copied();
    let last = candidates
        .iter()
        .filter(|c| c.2 == CueKind::Out)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .copied();
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    let mut selected: Vec<_> = first.into_iter().chain(last).collect();
    let spacing = (t.duration_sec / 24.).max(1.);
    for candidate in candidates {
        if selected.len() == 8 {
            break;
        }
        if selected.iter().all(|c| (c.0 - candidate.0).abs() > spacing) {
            selected.push(candidate);
        }
    }
    selected.sort_by(|a, b| a.0.total_cmp(&b.0));
    selected.dedup_by(|a, b| (a.0 - b.0).abs() < 0.05);
    selected
        .iter()
        .enumerate()
        .map(|(index, c)| Cue {
            index: index as u8,
            frame: (c.0 * t.sample_rate as f32)
                .round()
                .max(0.)
                .min((t.duration_sec * t.sample_rate as f32 - 1.).max(0.))
                as u64,
            kind: c.2,
            user_set: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_numbers_are_chronological_bounded_and_use_source_frames() {
        for sr in [44100, 48000] {
            let mut t = crate::features::analyze(mixless_protocol::TrackId(1), &[0.; 100], sr);
            t.duration_sec = 100.;
            t.phrase_boundaries = (0..20)
                .map(|i| mixless_protocol::PhraseBoundary {
                    time_sec: i as f32 * 5.,
                    confidence: 0.9,
                    novelty: 0.5,
                })
                .collect();
            let cues = automatic_cues(&t);
            assert_eq!(cues.len(), 8);
            assert!(cues.windows(2).all(|w| w[0].frame < w[1].frame));
            for (i, c) in cues.iter().enumerate() {
                assert_eq!(c.index, i as u8);
                assert!(!c.user_set);
                assert!(c.frame < (sr as u64) * 100);
                assert_eq!(c.frame % (sr as u64 * 5), 0);
            }
        }
    }
}
