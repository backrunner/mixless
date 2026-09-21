//! Candidate windows retain several musical alternatives before pair matching.
use mixless_protocol::{MixRegion, MixRegionKind as Kind, SectionLabel as S, TrackAnalysis};

pub(super) fn detect(t: &TrackAnalysis) -> Vec<MixRegion> {
    let audible: Vec<_> = t.bars.iter().filter(|b| b.rms > 0.001).collect();
    let (Some(first), Some(last)) = (audible.first(), audible.last()) else {
        return vec![];
    };
    let mut boundaries: Vec<_> = t
        .phrase_boundaries
        .iter()
        .filter(|p| p.confidence >= 0.5)
        .map(|p| (p.time_sec, p.confidence))
        .collect();
    boundaries.extend(
        t.sections
            .iter()
            .flat_map(|s| [(s.start_sec, 0.55), (s.end_sec, 0.55)]),
    );
    boundaries.extend([
        (first.start_sec, 0.5),
        (last.end_sec.min(t.duration_sec), 0.5),
    ]);
    let mut candidates = Vec::new();
    for kind in [Kind::In, Kind::Out] {
        for &(point, confidence) in &boundaries {
            let boundary = t
                .tempo
                .downbeats
                .iter()
                .copied()
                .min_by(|a, b| (a - point).abs().total_cmp(&(b - point).abs()));
            let anchor = boundary
                .filter(|b| (*b - point).abs() < 0.15)
                .unwrap_or(point);
            if kind == Kind::Out && mixless_protocol::drop_suspension(t, anchor).is_some() {
                continue;
            }
            let probe = if kind == Kind::In {
                anchor + 0.01
            } else {
                anchor - 0.01
            };
            let Some(index) = t
                .bars
                .iter()
                .position(|b| b.start_sec <= probe && probe < b.end_sec && b.rms > 0.001)
            else {
                continue;
            };
            let (start, end) = if kind == Kind::In {
                let end = t
                    .bars
                    .iter()
                    .skip(index)
                    .take(128)
                    .take_while(|b| b.rms > 0.001)
                    .last()
                    .unwrap()
                    .end_sec;
                (anchor, end.min(t.duration_sec))
            } else {
                let start = t.bars[..=index]
                    .iter()
                    .rev()
                    .take(128)
                    .take_while(|b| b.rms > 0.001)
                    .last()
                    .unwrap()
                    .start_sec;
                (start, anchor)
            };
            if end - start < 1. || start < 0. || end > t.duration_sec + 0.001 {
                continue;
            }
            let mut duration = 0.;
            let mut rms = 0.;
            let mut vocal = 0.;
            let mut kick = 0.;
            let mut chroma = [0f32; 12];
            // The first/last eight bars describe the actual reveal/exit, rather
            // than averaging a busy chorus into an otherwise quiet long window.
            let evidence: Vec<_> = if kind == Kind::In {
                t.bars.iter().skip(index).take(8).collect()
            } else {
                t.bars[..=index].iter().rev().take(8).collect()
            };
            for b in evidence {
                let seconds = (b.end_sec.min(end) - b.start_sec.max(start)).max(0.);
                duration += seconds;
                rms += b.rms * b.rms * seconds;
                vocal += b.vocal_presence.max(b.vocal_confidence.unwrap_or(0.)) * seconds;
                kick += b.kick_salience * seconds;
                for i in 0..12 {
                    chroma[i] += b.chroma[i] * seconds * b.rms;
                }
            }
            if duration <= 0. {
                continue;
            }
            let (_, camelot, key_confidence) = crate::features::key(&chroma);
            let section = t.bars[index].section;
            let structure = match (kind, section) {
                (Kind::In, S::Intro | S::Break | S::Breakdown | S::Drop)
                | (Kind::Out, S::Outro | S::Break | S::Chorus | S::Drop | S::BuildUp) => 0.12,
                (_, S::Silence) => continue,
                _ => 0.,
            };
            candidates.push(MixRegion {
                kind,
                start_sec: start,
                end_sec: end,
                anchor_sec: anchor,
                confidence: (confidence * 0.75 + structure + kick / duration * 0.15
                    - vocal / duration * 0.12)
                    .clamp(0., 1.),
                rms: (rms / duration).sqrt(),
                vocal_risk: vocal / duration,
                kick: kick / duration,
                camelot,
                key_confidence,
            });
        }
    }
    candidates.sort_by(|a, b| b.confidence.total_cmp(&a.confidence));
    let mut result: Vec<MixRegion> = Vec::new();
    for region in candidates {
        let spacing = 4. * t.tempo.meter_num.max(1) as f32 * 60. / t.tempo.global_bpm.max(20.);
        if result.iter().filter(|r| r.kind == region.kind).count() >= 8
            || result.iter().any(|r| {
                r.kind == region.kind && (r.anchor_sec - region.anchor_sec).abs() < spacing
            })
        {
            continue;
        }
        result.push(region);
    }
    result.sort_by(|a, b| a.anchor_sec.total_cmp(&b.anchor_sec));
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regions_are_audible_phrase_windows_and_keep_several_alternatives() {
        let sr = 22050;
        let mut pcm = vec![0f32; sr * 96 * 2];
        for beat in 4..184 {
            for i in 0..1100 {
                let p = beat * 11025 + i;
                let x = (i as f32 * 0.025).sin() * (1. - i as f32 / 1100.) * 0.5;
                pcm[p * 2] = x;
                pcm[p * 2 + 1] = x;
            }
        }
        let mut t = crate::features::analyze(mixless_protocol::TrackId(1), &pcm, sr as u32);
        // Supply annotated phrases to isolate region extraction from beat
        // detection: unaccented synthetic kicks cannot establish bar one.
        t.phrase_boundaries = t
            .bars
            .iter()
            .step_by(8)
            .map(|b| mixless_protocol::PhraseBoundary {
                time_sec: b.start_sec,
                confidence: 0.9,
                novelty: 0.4,
            })
            .collect();
        let regions = detect(&t);
        assert!(regions.iter().filter(|r| r.kind == Kind::In).count() > 1);
        assert!(regions.iter().filter(|r| r.kind == Kind::Out).count() > 1);
        for r in regions {
            assert!(
                r.start_sec >= 0.
                    && r.end_sec <= t.duration_sec
                    && r.start_sec < r.end_sec
                    && r.rms > 0.001
            );
        }
        let silent = crate::features::analyze(
            mixless_protocol::TrackId(2),
            &vec![0.; sr * 4 * 2],
            sr as u32,
        );
        assert!(detect(&silent).is_empty());
    }
}
