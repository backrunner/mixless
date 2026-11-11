//! Comparable musical intensity from measured spectrum and rhythmic activity.
//! This is an ordering descriptor, not LUFS or a perceptual ground truth.
use crate::{BarFeature, TrackAnalysis};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct MusicalEnergy {
    /// Broadband-weighted level, dB relative to the track's active RMS.
    pub body_db: f32,
    /// Flux occupancy weighted by low-frequency rhythmic activity, 0..1.
    pub activity: f32,
    /// Combined relative intensity in dB-like units.
    pub drive_db: f32,
}

pub fn energy_reference(track: &TrackAnalysis) -> f32 {
    let mut levels: Vec<_> = track
        .bars
        .iter()
        .map(|b| b.rms)
        .filter(|r| r.is_finite() && *r > 0.001)
        .collect();
    levels.sort_by(f32::total_cmp);
    levels
        .get(levels.len().saturating_sub(1) * 3 / 4)
        .copied()
        .unwrap_or(1.)
}

pub fn bar_energy(bar: &BarFeature, reference: f32) -> MusicalEnergy {
    let bands =
        [bar.low_db, bar.mid_db, bar.high_db].map(|v| 10f32.powf(v.clamp(-120., 30.) / 10.));
    let total = bands.iter().sum::<f32>().max(1e-12);
    // Sub-bass contributes physical weight but cannot stand in for broadband
    // presence. Keep every band; avoid treating brighter as always louder.
    let body = (0.2 * bands[0] + bands[1] + bands[2]) / total;
    let body_db = 20. * (bar.rms.max(1e-6) / reference.max(1e-6)).log10() + 10. * body.log10();
    // onset_density is active flux frames per second (86.13 maximum), not a
    // kick count or BPM. It is invariant to half/double tempo interpretations.
    let activity = (bar.onset_density / (22050. / 256.)).clamp(0., 1.)
        * (0.35 + 0.65 * bar.kick_salience.clamp(0., 1.));
    let drive_db = body_db + 10. * (0.25 + 0.75 * activity).log10();
    MusicalEnergy {
        body_db,
        activity,
        drive_db,
    }
}

/// Duration-weighted energy over any section or exact handoff window.
/// Empty/unknown measurements remain unavailable, not a fabricated zero.
pub fn musical_energy(track: &TrackAnalysis, start: f32, end: f32) -> Option<MusicalEnergy> {
    let reference = energy_reference(track);
    let mut totals = [0f32; 4];
    for b in &track.bars {
        let width = (end.min(b.end_sec) - start.max(b.start_sec)).max(0.);
        if width == 0. || !b.rms.is_finite() {
            continue;
        }
        let e = bar_energy(b, reference);
        totals[0] += width;
        totals[1] += width * 10f32.powf(e.body_db / 10.);
        totals[2] += width * e.activity;
        totals[3] += width * 10f32.powf(e.drive_db / 10.);
    }
    (totals[0] > 0.).then(|| MusicalEnergy {
        body_db: 10. * (totals[1] / totals[0]).max(1e-12).log10(),
        activity: totals[2] / totals[0],
        drive_db: 10. * (totals[3] / totals[0]).max(1e-12).log10(),
    })
}

/// Upper-quartile active intensity: compare the body a track builds towards,
/// rather than how much of its arrangement happens to be a quiet breakdown.
pub fn track_energy(track: &TrackAnalysis) -> Option<f32> {
    let reference = energy_reference(track);
    let mut values: Vec<_> = track
        .bars
        .iter()
        .filter(|b| b.rms >= reference * 0.5)
        .map(|b| bar_energy(b, reference).drive_db)
        .collect();
    values.sort_by(f32::total_cmp);
    values.get(values.len().saturating_sub(1) * 3 / 4).copied()
}
