//! Refine a coarse clock against measured attacks in overlapping phrases.
//! Small tempo changes matter: one percent drift is a whole beat in 100 beats.
use super::{Frame, tempo};

fn score(frames: &[Frame], bpm: f32, phase: f32) -> f32 {
    let period = 60. / bpm;
    frames
        .iter()
        .map(|f| {
            let distance = (f.time - phase + period * 0.5).rem_euclid(period) - period * 0.5;
            f.onset * (1. - distance.abs() / 0.035).max(0.)
        })
        .sum()
}

/// Keep supported anchors only. Interpolated spans must prove their own
/// confidence, recomputed from the resulting timestamps by the caller.
pub(super) fn refine(frames: &[Frame], beats: &mut [f32], bpms: &mut [f32]) {
    if beats.len() < 32 {
        return;
    }
    let original = beats.to_vec();
    let mut models = Vec::new();
    for i in (0..beats.len()).step_by(16) {
        let lo = i.saturating_sub(16);
        let hi = (i + 32).min(beats.len() - 1);
        let start = frames.partition_point(|f| f.time < original[lo]);
        let end = frames.partition_point(|f| f.time < original[hi]);
        let local = &frames[start..end];
        if local.len() < 128 {
            continue;
        }
        let base = bpms[(i / 16).min(bpms.len() - 1)];
        let mut best = (0., base, original[i]);
        // Search close periods, not metrical aliases. Broad tempo changes are
        // still owned by the coarse estimator, and need its independent proof.
        for j in -80..=80 {
            let bpm = base * (1. + j as f32 * 0.0005);
            let (_, bin) = tempo::fold(local, bpm);
            let phase = (bin as f32 + 0.5) / 64. * 60. / bpm;
            let value = score(local, bpm, phase);
            if value > best.0 {
                best = (value, bpm, phase);
            }
        }
        let (_, bpm, phase) = best;
        let support = tempo::alignment_confidence(local, bpm, phase);
        // Both halves must support this same pulse. A single fill or a chord
        // change cannot move the clock for the rest of a recording.
        let middle = local.len() / 2;
        if support < 0.25
            || tempo::alignment_confidence(&local[..middle], bpm, phase) < 0.2
            || tempo::alignment_confidence(&local[middle..], bpm, phase) < 0.2
        {
            continue;
        }
        models.push((i, bpm, phase));
    }
    // Successive overlapping windows must agree in both period and phase.
    // This rejects one-off fills and a dense but unmeasured synthesized clock.
    let mut trusted = vec![false; models.len()];
    for (i, w) in models.windows(3).enumerate() {
        if w.windows(2).all(|p| {
            let period = 60. / p[0].1;
            let time = original[p[1].0];
            let a = (time - p[0].2) * p[0].1 / 60.;
            let b = (time - p[1].2) * p[1].1 / 60.;
            p[1].0 - p[0].0 == 16
                && (p[1].1 / p[0].1 - 1.).abs() < 0.003
                && ((a - b + 0.5).rem_euclid(1.) - 0.5).abs() * period < 0.035
        }) {
            trusted[i..i + 3].fill(true);
        }
    }
    let mut anchors = Vec::new();
    for ((i, bpm, phase), trusted) in models.into_iter().zip(trusted) {
        if !trusted {
            continue;
        }
        let period = 60. / bpm;
        let predicted = anchors.last().map_or(
            original[i],
            |&(index, sec, previous): &(usize, f32, f32)| {
                sec + (i - index) as f32 * 120. / (previous + bpm)
            },
        );
        let sec = phase + ((predicted - phase) / period).round() * period;
        // Alternating kick/snare accents can choose the opposite half-beat
        // in another window. Do not turn that metrical ambiguity into a
        // fabricated tempo excursion between two otherwise steady phrases.
        if !anchors.is_empty() && (sec - predicted).abs() > period * 0.2 {
            continue;
        }
        anchors.push((i, sec, bpm));
    }
    // Do not replace a stable global clock for sub-hop local improvements.
    // Three overlapping phrases must agree on a material period correction.
    let sustained = anchors.windows(3).any(|w| {
        w.windows(2)
            .all(|p| p[1].0 - p[0].0 == 16 && (p[1].2 / p[0].2 - 1.).abs() < 0.002)
            && w.iter()
                .all(|a| (a.2 / bpms[(a.0 / 16).min(bpms.len() - 1)] - 1.).abs() >= 0.003)
    });
    if !sustained {
        return;
    }
    for pair in anchors.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        // Interpolate the clock only; each passage must still prove its own
        // pulse confidence. A long unmeasured span cannot inherit confidence.
        if b.1 <= a.1 {
            continue;
        }
        let period = (b.1 - a.1) / (b.0 - a.0) as f32;
        if (period * a.2 / 60. - 1.).abs() > 0.04 || (period * b.2 / 60. - 1.).abs() > 0.04 {
            continue;
        }
        for (j, beat) in beats.iter_mut().enumerate().take(b.0 + 1).skip(a.0) {
            *beat = a.1 + (j - a.0) as f32 * period;
        }
    }
    let first = anchors[0];
    let last = anchors[anchors.len() - 1];
    for (i, beat) in beats.iter_mut().enumerate() {
        if i < first.0 {
            *beat = first.1 - (first.0 - i) as f32 * 60. / first.2;
        }
        if i > last.0 {
            *beat = last.1 + (i - last.0) as f32 * 60. / last.2;
        }
    }
    if beats[0] < 0. || beats.windows(2).any(|w| w[1] <= w[0]) {
        beats.copy_from_slice(&original);
        return;
    }
    for (i, bpm) in bpms.iter_mut().enumerate() {
        let lo = i * 16;
        let hi = (lo + 16).min(beats.len() - 1);
        if hi > lo {
            *bpm = 60. * (hi - lo) as f32 / (beats[hi] - beats[lo]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sustained_small_tempo_change_keeps_the_observed_beat_phase() {
        for (gain, shift) in [(0.1, 0.), (1., 0.173), (4., 0.61)] {
            let mut truth = vec![0.08f32 + shift];
            while *truth.last().unwrap() < 100. {
                let last = *truth.last().unwrap();
                truth.push(last + 60. / if last < 48. + shift { 180. } else { 178. });
            }
            let frames: Vec<_> = (0..10000)
                .map(|i| {
                    let time = i as f32 * 0.01;
                    let distance = truth
                        .iter()
                        .map(|b| (time - b).abs())
                        .fold(f32::INFINITY, f32::min);
                    Frame {
                        time,
                        onset: gain * (1. - distance / 0.025).max(0.),
                        ..Default::default()
                    }
                })
                .collect();
            let mut beats: Vec<_> = (0..290)
                .map(|i| 0.08 + shift + i as f32 * 60. / 178.2)
                .collect();
            let mut bpms = vec![178.2; beats.len().div_ceil(16)];
            refine(&frames, &mut beats, &mut bpms);
            for i in 16..270 {
                assert!(
                    (beats[i] - truth[i]).abs() < 0.065,
                    "beat {i}: {} vs {}",
                    beats[i],
                    truth[i]
                );
            }
        }
    }

    #[test]
    fn a_short_fill_or_diffuse_flux_cannot_retime_the_recording() {
        for diffuse in [false, true] {
            let frames: Vec<_> = (0..10000)
                .map(|i| {
                    let time = i as f32 * 0.01;
                    Frame {
                        time,
                        onset: if diffuse {
                            1.
                        } else if (20. ..21.).contains(&time) {
                            (1. - (time * 2.).fract() / 0.05).max(0.)
                        } else {
                            0.
                        },
                        ..Default::default()
                    }
                })
                .collect();
            let original: Vec<_> = (0..190).map(|i| 0.08 + i as f32 * 60. / 119.).collect();
            let mut beats = original.clone();
            let mut bpms = vec![119.; beats.len().div_ceil(16)];
            refine(&frames, &mut beats, &mut bpms);
            assert_eq!(beats, original);
            assert!(bpms.iter().all(|b| *b == 119.));
        }
    }
}
