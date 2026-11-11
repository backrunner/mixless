//! Keep an early complete passage available to the next-transition forecast.
use mixless_protocol::{MixPlan, TrackAnalysis};

pub(super) fn select(mut candidates: Vec<(f32, MixPlan)>, b: &TrackAnalysis) -> Vec<MixPlan> {
    candidates.sort_by(|a, b| b.0.total_cmp(&a.0));
    let entries: Vec<_> = candidates
        .iter()
        .map(|(_, p)| crate::established_entry(p, b))
        .collect();
    // Several higher-scoring late sections or pitch variants can all owe the
    // same late peak. Reserve one of the existing three slots for the best
    // candidate that still delivers the earliest complete peak, so lookahead
    // can compare its actual departure instead of never seeing that option.
    let earliest = entries
        .iter()
        .enumerate()
        .filter_map(|(i, entry)| crate::drops::owed_peak(b, *entry).map(|peak| (i, peak.1)))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i);
    let mut selected = Vec::new();
    if !candidates.is_empty() {
        selected.push(0);
    }
    if let Some(i) = earliest.filter(|i| !selected.contains(i)) {
        selected.push(i);
    }
    let section = |sec| {
        b.sections
            .iter()
            .position(|s| sec >= s.start_sec - 0.05 && sec < s.end_sec - 0.05)
    };
    for (i, (_, p)) in candidates.iter().enumerate() {
        if selected.len() == 3 {
            break;
        }
        if selected.iter().any(|&j| {
            let old = &candidates[j].1;
            section(entries[j]) == section(entries[i])
                && (old.incoming_offset_end.rate - p.incoming_offset_end.rate).abs() < 0.001
                && (old.incoming_offset_end.pitch_semitones - p.incoming_offset_end.pitch_semitones)
                    .abs()
                    < 0.05
        }) {
            continue;
        }
        selected.push(i);
    }
    selected.sort_unstable();
    candidates
        .into_iter()
        .enumerate()
        .filter_map(|(i, (_, p))| selected.contains(&i).then_some(p))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{MixPlanSummary, Section, SectionLabel as S, StrategyId};

    fn candidate(b: &TrackAnalysis, entry: f32, score: f32, pitch: f32) -> (f32, MixPlan) {
        let mut p = MixPlan {
            t_in_b: entry,
            summary: Some(MixPlanSummary {
                pair: (b.track_id, b.track_id),
                strategy: StrategyId::DryCut,
                score,
                used_fallback: false,
                length_bars: 0,
            }),
            ..Default::default()
        };
        p.incoming_offset_end.pitch_semitones = pitch;
        (score, p)
    }

    #[test]
    fn a_lower_current_score_can_preserve_an_earlier_complete_peak() {
        let mut b = crate::tests::track(2, 120., "8A", S::Intro, 64, 0.8, 0.2);
        b.sections = [
            (0., 16., S::Intro),
            (16., 48., S::Drop),
            (48., 64., S::Break),
            (64., 128., S::Drop),
        ]
        .map(|(start_sec, end_sec, label)| Section {
            start_sec,
            end_sec,
            label,
        })
        .to_vec();
        let choices = select(
            vec![
                candidate(&b, 64., 0.9, 0.),
                candidate(&b, 18., 0.8, 0.),
                candidate(&b, 64., 0.7, 1.),
                candidate(&b, 8., 0.6, 0.),
            ],
            &b,
        );
        assert_eq!(choices.len(), 3);
        assert_eq!(choices[0].t_in_b, 64.);
        assert!(choices.iter().any(|p| p.t_in_b == 8.));
        assert_eq!(crate::drops::owed_peak(&b, 8.).unwrap().1, 48.);
        assert_eq!(crate::drops::owed_peak(&b, 18.).unwrap().1, 128.);
    }

    #[test]
    fn absent_peaks_keep_the_ranked_distinct_states_and_do_not_invent_an_entrance() {
        let b = crate::tests::track(2, 120., "8A", S::Intro, 64, 0.8, 0.2);
        let choices = select(
            (0..5)
                .map(|i| candidate(&b, 20., 0.9 - i as f32 * 0.1, i as f32))
                .collect(),
            &b,
        );
        assert_eq!(choices.len(), 3);
        assert_eq!(
            choices
                .iter()
                .map(|p| p.incoming_offset_end.pitch_semitones)
                .collect::<Vec<_>>(),
            vec![0., 1., 2.]
        );
        assert!(select(Vec::new(), &b).is_empty());
    }
}
