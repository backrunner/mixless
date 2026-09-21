//! Sparse structural suggestions. Eight slots are capacity, never a target.
use mixless_protocol::{Cue, CueKind, SectionLabel as S, TrackAnalysis};

pub fn automatic_cues(t: &TrackAnalysis) -> Vec<Cue> {
    if t.sample_rate == 0 || !t.duration_sec.is_finite() || t.duration_sec <= 0. {
        return vec![];
    }
    let audible: Vec<_> = t
        .bars
        .iter()
        .filter(|b| b.rms > 0.001 && b.section != S::Silence)
        .collect();
    let (Some(first), Some(last)) = (audible.first(), audible.last()) else {
        return vec![];
    };
    let entry = first.start_sec;
    let exit = mixless_protocol::peak_ranges(t)
        .into_iter()
        .find(|&(start, end)| {
            end - start >= 8.
                && end > entry + 16.
                && mixless_protocol::drop_suspension(t, end).is_none()
        })
        .map_or(last.end_sec.min(t.duration_sec), |(_, end)| end);
    let mut selected = vec![(entry, 1., CueKind::In), (exit, 1., CueKind::Out)];
    let spacing = (8. * 60. / t.tempo.global_bpm.max(20.)).max(1.);
    let mut candidates = Vec::new();
    for pair in t.sections.windows(2) {
        let (previous, next) = (&pair[0], &pair[1]);
        if previous.label == next.label
            || !matches!(
                next.label,
                S::Drop | S::Chorus | S::BuildUp | S::Breakdown | S::Outro
            )
        {
            continue;
        }
        let point = next.start_sec;
        let evidence = t
            .phrase_boundaries
            .iter()
            .filter(|p| (p.time_sec - point).abs() < 0.15)
            .filter(|p| p.confidence >= 0.65 && p.novelty >= 0.28)
            .map(|p| p.confidence * p.novelty.min(1.))
            .fold(0., f32::max);
        if evidence > 0. && next.end_sec - next.start_sec >= spacing {
            candidates.push((point, evidence, CueKind::Hot));
        }
    }
    candidates.sort_by(|a, b| b.1.total_cmp(&a.1));
    for candidate in candidates {
        if selected.len() >= 8 {
            break;
        }
        if candidate.0 >= entry
            && candidate.0 < last.end_sec
            && selected
                .iter()
                .all(|c| (c.0 - candidate.0).abs() >= spacing)
        {
            selected.push(candidate);
        }
    }
    selected.sort_by(|a, b| a.0.total_cmp(&b.0));
    selected.dedup_by(|a, b| (a.0 - b.0).abs() < 0.05);
    selected
        .into_iter()
        .enumerate()
        .map(|(index, (time, _, kind))| Cue {
            index: index as u8,
            frame: (time * t.sample_rate as f32)
                .round()
                .clamp(0., (t.duration_sec * t.sample_rate as f32 - 1.).max(0.))
                as u64,
            kind,
            user_set: false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn silence_has_no_cues_and_metrical_subdivisions_do_not_fill_pads() {
        for sr in [44100, 48000] {
            let mut t = crate::features::analyze(
                mixless_protocol::TrackId(1),
                &vec![0.; sr * 12 * 2],
                sr as u32,
            );
            assert!(automatic_cues(&t).is_empty());
            for bar in &mut t.bars {
                bar.rms = 0.2;
                bar.section = S::Unknown;
            }
            t.phrase_boundaries = (0..12)
                .map(|i| mixless_protocol::PhraseBoundary {
                    time_sec: i as f32,
                    confidence: 0.9,
                    novelty: 0.,
                })
                .collect();
            let cues = automatic_cues(&t);
            assert_eq!(cues.len(), 2);
            assert!(cues.windows(2).all(|c| c[0].frame < c[1].frame));
            assert_eq!(cues[0].kind, CueKind::In);
            assert_eq!(cues[1].kind, CueKind::Out);
            assert!(cues[1].frame < sr as u64 * 12);
        }
    }
    #[test]
    fn only_evidenced_structural_changes_add_hot_cues() {
        let mut t = crate::features::analyze(
            mixless_protocol::TrackId(1),
            &vec![0.; 22050 * 64 * 2],
            22050,
        );
        for bar in &mut t.bars {
            bar.rms = 0.2;
            bar.section = S::Unknown;
        }
        t.tempo.global_bpm = 120.;
        t.sections = vec![
            mixless_protocol::Section {
                start_sec: 0.,
                end_sec: 16.,
                label: S::Intro,
            },
            mixless_protocol::Section {
                start_sec: 16.,
                end_sec: 32.,
                label: S::Drop,
            },
            mixless_protocol::Section {
                start_sec: 32.,
                end_sec: 48.,
                label: S::Breakdown,
            },
            mixless_protocol::Section {
                start_sec: 48.,
                end_sec: 64.,
                label: S::Drop,
            },
        ];
        t.phrase_boundaries = vec![mixless_protocol::PhraseBoundary {
            time_sec: 16.,
            confidence: 0.9,
            novelty: 0.7,
        }];
        let cues = automatic_cues(&t);
        assert_eq!(cues.len(), 3);
        assert_eq!(
            cues.iter().find(|c| c.kind == CueKind::Out).unwrap().frame,
            32 * 22050
        );
        assert!(!cues.iter().any(|c| c.frame == 48 * 22050));
        // Adjacent peak variations share one exit even when a variation is
        // shorter than the planner's minimum peak length.
        t.sections[1].end_sec = 20.;
        t.sections.insert(
            2,
            mixless_protocol::Section {
                start_sec: 20.,
                end_sec: 32.,
                label: S::Chorus,
            },
        );
        assert_eq!(
            automatic_cues(&t)
                .iter()
                .find(|c| c.kind == CueKind::Out)
                .unwrap()
                .frame,
            32 * 22050
        );
    }
}
