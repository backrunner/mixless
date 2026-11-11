//! Evidence for a locally repeated vocal sample, not a lyric classifier.
//! Short notes alone also describe rap, staccato singing and transcription
//! fragments. Require the vocal envelope, spectral shape AND note sequence to
//! recur. Missing evidence stays unknown; a whole song never inherits a label.
use crate::{StemFrame, StemKind, StemNote, TrackAnalysis};

#[derive(Clone, Copy, Debug)]
pub struct VocalPattern {
    pub start_sec: f32,
    pub end_sec: f32,
    pub period_sec: f32,
    pub envelope_similarity: f32,
    pub spectral_distance: f32,
    pub note_similarity: f32,
}

/// Recognize short, articulated vocal motifs over at least four repetitions.
/// Thresholds describe evidence, not calibrated classification probabilities:
/// 0.92 envelope correlation, <=15% spectral redistribution, >=75% repeated
/// note onsets, and predominantly sub-pulse notes. Repeated full verses,
/// sustained vowels, regular syllable timing alone and separator leakage do
/// not authorize treating a singer as a rhythmic layer.
pub fn vocal_pattern(t: &TrackAnalysis, sec: f32) -> Option<VocalPattern> {
    let stems = t.stems.as_ref()?;
    let pulse = local_pulse(t, sec)?;
    if !sec.is_finite() || stems.frames.len() < 3 {
        return None;
    }
    let mut best: Option<VocalPattern> = None;
    for pulses in [0.5, 1., 2., 4.] {
        let span = (4f32 * pulses).max(8.) * pulse;
        let (start, end) = (sec - span * 0.5, sec + span * 0.5);
        if start < 0. || end > t.duration_sec {
            continue;
        }
        // A changing clock needs a time-warped fingerprint. Until available,
        // do not infer repetition across more than 5% local tempo movement.
        if [start, end]
            .into_iter()
            .any(|at| local_pulse(t, at).is_none_or(|p| (p / pulse - 1.).abs() > 0.05))
        {
            continue;
        }
        let notes: Vec<_> = stems.notes[..stems.notes.partition_point(|n| n.start_sec < end)]
            .iter()
            .filter(|n| n.stem == StemKind::Vocals && n.confidence >= 0.45 && n.end_sec > start)
            .collect();
        if notes.len() < 6
            || notes
                .iter()
                .any(|n| !n.end_sec.is_finite() || n.end_sec - n.start_sec > pulse)
            || notes
                .iter()
                .filter(|n| n.end_sec - n.start_sec <= 0.75 * pulse)
                .count()
                * 5
                < notes.len() * 4
        {
            continue;
        }
        // Work in source seconds. Normalizing the local pulse keeps equivalent
        // half/double grids identical; the fingerprint does not require a
        // confident downbeat. A grid is still required separately for mixing.
        let count = (span / pulse * 16.).round() as usize;
        let step = span / count as f32;
        let samples: Option<Vec<_>> = (0..count)
            .map(|i| sample(&stems.frames, start + (i as f32 + 0.5) * step))
            .collect();
        let Some(samples) = samples else { continue };
        let peak = samples.iter().map(|s| s.amplitude).fold(0., f32::max);
        if peak <= 0.
            || samples.iter().filter(|s| s.present).count() * 4 < count
            || samples.iter().filter(|s| s.amplitude <= peak * 0.5).count() * 5 < count
        {
            continue;
        }
        for scale in [1., 0.98, 1.02] {
            let period = pulses * pulse * scale;
            let pairs: Option<Vec<_>> = samples
                .iter()
                .enumerate()
                .take_while(|(i, _)| start + (*i as f32 + 0.5) * step + period < end)
                .map(|(i, a)| {
                    sample(&stems.frames, start + (i as f32 + 0.5) * step + period).map(|b| (*a, b))
                })
                .collect();
            let Some(pairs) = pairs else { continue };
            let envelope = correlation(&pairs);
            if envelope < 0.92
                || correlation(&pairs[..pairs.len() / 2]) < 0.85
                || correlation(&pairs[pairs.len() / 2..]) < 0.85
            {
                continue;
            }
            let (mut weight, mut distance) = (0., 0.);
            for (a, b) in &pairs {
                let w = a.amplitude.min(b.amplitude).powi(2);
                weight += w;
                distance += w * a
                    .bands
                    .iter()
                    .zip(b.bands)
                    .map(|(x, y)| (x - y).abs())
                    .sum::<f32>();
            }
            let spectral = distance / weight.max(f32::MIN_POSITIVE);
            let note_similarity = repeated_notes(&notes, start, end, period, pulse);
            // A repeating neighborhood cannot authorize removing a different
            // word at the actual edit point. Check the central slice too.
            let local_notes = notes
                .iter()
                .filter(|n| n.start_sec < sec + pulse * 0.25 && n.end_sec > sec - pulse * 0.25)
                .all(|n| {
                    [-period, period]
                        .into_iter()
                        .any(|lag| notes.iter().any(|m| same_note(n, m, lag, pulse)))
                });
            let local_color = pairs
                .iter()
                .enumerate()
                .filter(|(i, _)| (start + (*i as f32 + 0.5) * step - sec).abs() <= pulse * 0.25)
                .all(|(_, (a, b))| {
                    a.amplitude < peak * 0.25
                        || (b.amplitude >= peak * 0.125
                            && a.bands
                                .iter()
                                .zip(b.bands)
                                .map(|(x, y)| (x - y).abs())
                                .sum::<f32>()
                                <= 0.3)
                });
            if spectral > 0.15 || note_similarity < 0.75 || !local_notes || !local_color {
                continue;
            }
            let pattern = VocalPattern {
                start_sec: start,
                end_sec: end,
                period_sec: period,
                envelope_similarity: envelope,
                spectral_distance: spectral,
                note_similarity,
            };
            if best.is_none_or(|p| envelope > p.envelope_similarity) {
                best = Some(pattern);
            }
        }
    }
    best
}

fn local_pulse(t: &TrackAnalysis, sec: f32) -> Option<f32> {
    let beats = &t.tempo.beats;
    let mut bpm = if beats.len() >= 2 {
        let i = beats
            .partition_point(|b| *b <= sec)
            .saturating_sub(1)
            .min(beats.len() - 2);
        60. / (beats[i + 1] - beats[i])
    } else {
        t.tempo.global_bpm
    };
    if !bpm.is_finite() || !(20. ..=400.).contains(&bpm) {
        return None;
    }
    while bpm > 160. {
        bpm *= 0.5;
    }
    while bpm < 80. {
        bpm *= 2.;
    }
    Some(60. / bpm)
}

#[derive(Clone, Copy)]
struct Sample {
    amplitude: f32,
    bands: [f32; 3],
    present: bool,
}

fn sample(frames: &[StemFrame], sec: f32) -> Option<Sample> {
    let i = frames
        .partition_point(|f| (f.start_sec + f.end_sec) * 0.5 <= sec)
        .saturating_sub(1);
    let a = frames.get(i)?;
    let b = frames.get(i + 1)?;
    if sec < a.start_sec || sec > b.end_sec || (b.start_sec - a.end_sec).abs() > 0.001 {
        return None;
    }
    let center = (a.start_sec + a.end_sec) * 0.5;
    let width = (b.start_sec + b.end_sec) * 0.5 - center;
    if width <= 0. || width > 0.1 {
        return None;
    }
    let u = ((sec - center) / width).clamp(0., 1.);
    let mix = |x: f32, y: f32| x + (y - x) * u;
    let amplitude = mix(a.rms[0], b.rms[0]);
    let mut bands: [f32; 3] = std::array::from_fn(|i| {
        mix(
            10f32.powf(a.band_db[0][i] / 10.),
            10f32.powf(b.band_db[0][i] / 10.),
        )
    });
    let sum: f32 = bands.iter().sum();
    if !amplitude.is_finite() || amplitude < 0. || !sum.is_finite() || sum <= 0. {
        return None;
    }
    for band in &mut bands {
        *band /= sum;
    }
    let other = mix(
        a.rms[1].powi(2) + a.rms[2].powi(2),
        b.rms[1].powi(2) + b.rms[2].powi(2),
    );
    Some(Sample {
        amplitude,
        bands,
        present: mix(a.vocal_activity, b.vocal_activity) >= 0.2
            && amplitude.powi(2) > other * 0.0001,
    })
}

fn correlation(pairs: &[(Sample, Sample)]) -> f32 {
    if pairs.len() < 8 {
        return 0.;
    }
    let n = pairs.len() as f32;
    let ma = pairs.iter().map(|p| p.0.amplitude).sum::<f32>() / n;
    let mb = pairs.iter().map(|p| p.1.amplitude).sum::<f32>() / n;
    let (mut aa, mut bb, mut ab) = (0., 0., 0.);
    for (a, b) in pairs {
        aa += (a.amplitude - ma).powi(2);
        bb += (b.amplitude - mb).powi(2);
        ab += (a.amplitude - ma) * (b.amplitude - mb);
    }
    // A held vowel with minor tremolo is not an articulated sample sequence.
    if aa < n * (ma * 0.25).powi(2) || bb < n * (mb * 0.25).powi(2) {
        return 0.;
    }
    ab / (aa * bb).sqrt().max(f32::MIN_POSITIVE)
}

fn repeated_notes(notes: &[&StemNote], start: f32, end: f32, period: f32, pulse: f32) -> f32 {
    let mut count = 0;
    let mut matched = 0;
    for n in notes
        .iter()
        .filter(|n| n.start_sec >= start && n.end_sec + period < end)
    {
        count += 1;
        matched += usize::from(notes.iter().any(|m| same_note(n, m, period, pulse)));
    }
    if count < 4 {
        0.
    } else {
        matched as f32 / count as f32
    }
}

fn same_note(n: &StemNote, m: &StemNote, period: f32, pulse: f32) -> bool {
    m.midi % 12 == n.midi % 12
        && (m.start_sec - n.start_sec - period).abs() <= pulse * 0.1
        && ((m.end_sec - m.start_sec) - (n.end_sec - n.start_sec)).abs() <= pulse * 0.15
}

#[cfg(test)]
mod tests;
