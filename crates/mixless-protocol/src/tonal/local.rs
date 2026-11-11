use super::{candidate, certainty, correlations, KeyCandidate};
use crate::TrackAnalysis;

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct TonalProfile {
    pub chroma: [f32; 12],
    pub candidates: Vec<KeyCandidate>,
    pub confidence: f32,
    pub harmonic_confidence: f32,
    /// Fraction of the requested time with usable observations. Silence and
    /// missing observations cannot borrow certainty from the rest of the song.
    pub coverage: f32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TonalWindow {
    pub start_sec: f32,
    pub end_sec: f32,
    pub profile: TonalProfile,
}

/// Exact clipped source interval. Old analyses can supply coarser bar evidence;
/// never substitute a whole-song label for a silent or missing local profile.
pub fn tonal_profile(t: &TrackAnalysis, start: f32, end: f32) -> TonalProfile {
    if !start.is_finite() || !end.is_finite() || end <= start {
        return TonalProfile::default();
    }
    let mut chroma = [0.; 12];
    let mut covered = 0.;
    let mut add = |s: f32, e: f32, c: &[f32; 12]| {
        let seconds = (e.min(end) - s.max(start)).max(0.);
        let sum = c.iter().sum::<f32>();
        if seconds <= 0.
            || !sum.is_finite()
            || sum <= 1e-8
            || c.iter().any(|v| !v.is_finite() || *v < 0.)
        {
            return;
        }
        covered += seconds;
        for i in 0..12 {
            chroma[i] += c[i] / sum * seconds;
        }
    };
    if t.tonal_frames.is_empty() {
        for b in t
            .bars
            .iter()
            .skip_while(|b| b.end_sec <= start)
            .take_while(|b| b.start_sec < end)
        {
            if b.rms > 0. {
                add(b.start_sec, b.end_sec, &b.chroma);
            }
        }
    } else {
        let first = t.tonal_frames.partition_point(|f| f.end_sec <= start);
        for f in t.tonal_frames[first..]
            .iter()
            .take_while(|f| f.start_sec < end)
        {
            add(f.start_sec, f.end_sec, &f.chroma);
        }
    }
    let coverage = (covered / (end - start)).clamp(0., 1.);
    if covered > 0. {
        for v in &mut chroma {
            *v /= covered;
        }
    }
    let scores = correlations(&chroma);
    let confidence = certainty(&scores, false) * coverage;
    let harmonic_confidence = certainty(&scores, true) * coverage;
    let candidates = scores
        .into_iter()
        .take(3)
        .map(|(s, m, r)| candidate(s, m, r))
        .collect();
    TonalProfile {
        chroma,
        candidates,
        confidence,
        harmonic_confidence,
        coverage,
    }
}

/// Bound the statistical context to 16 source seconds, matching the existing
/// detector's context, and split at every section boundary. This is an analysis
/// scale, not a claim that a musical phrase lasts sixteen seconds. No BPM/grid
/// interpretation changes the audio included in a local key estimate.
pub fn tonal_windows(t: &TrackAnalysis, start: f32, end: f32) -> Vec<TonalWindow> {
    tonal_intervals(t, start, end)
        .into_iter()
        .map(|(s, e)| TonalWindow {
            start_sec: s,
            end_sec: e,
            profile: tonal_profile(t, s, e),
        })
        .collect()
}

/// Shared context partition, also used to align two source clocks before
/// computing profiles. Constructing boundaries does not score unused spectra.
pub fn tonal_intervals(t: &TrackAnalysis, start: f32, end: f32) -> Vec<(f32, f32)> {
    if !start.is_finite() || !end.is_finite() || end <= start {
        return vec![];
    }
    let mut edges = vec![start, end];
    edges.extend(
        t.sections
            .iter()
            .flat_map(|s| [s.start_sec, s.end_sec])
            .filter(|s| *s > start && *s < end),
    );
    edges.sort_by(f32::total_cmp);
    edges.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    let mut result = Vec::new();
    for pair in edges.windows(2) {
        let count = ((pair[1] - pair[0]) / 16.).ceil().max(1.) as usize;
        for i in 0..count {
            // Equal pieces avoid a tiny final sliver deciding a long section.
            let s = pair[0] + (pair[1] - pair[0]) * i as f32 / count as f32;
            let e = pair[0] + (pair[1] - pair[0]) * (i + 1) as f32 / count as f32;
            result.push((s, e));
        }
    }
    result
}

/// Dominant observed local key and conservative agreement over the whole
/// requested interval. A mixture of keys must not invent a third, unobserved
/// key by averaging their chroma first. Used for section/region summaries;
/// overlap planning compares the individual windows in temporal order.
pub fn local_tonality(
    t: &TrackAnalysis,
    start: f32,
    end: f32,
) -> (Option<String>, Option<String>, f32) {
    let windows = tonal_windows(t, start, end);
    let mut votes: Vec<(&KeyCandidate, f32)> = vec![];
    for w in &windows {
        if let Some(c) = w.profile.candidates.first() {
            let weight = (w.end_sec - w.start_sec) * w.profile.confidence;
            if let Some((_, sum)) = votes.iter_mut().find(|(k, _)| k.camelot == c.camelot) {
                *sum += weight;
            } else {
                votes.push((c, weight));
            }
        }
    }
    let Some((best, weight)) = votes.into_iter().max_by(|a, b| a.1.total_cmp(&b.1)) else {
        return (None, None, 0.);
    };
    (
        Some(best.key.clone()),
        Some(best.camelot.clone()),
        (weight / (end - start)).clamp(0., 1.),
    )
}
