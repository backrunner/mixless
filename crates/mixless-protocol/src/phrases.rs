//! Shared phrase semantics for cue suggestions and transition planning.
use crate::{SectionLabel as S, TrackAnalysis};

/// Merge adjacent peak variations before applying the minimum duration. A
/// timbre or vocal change inside a drop does not finish its musical phrase.
pub fn peak_ranges(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    let mut peaks: Vec<(f32, f32)> = Vec::new();
    if track.sections.len() < 2 {
        return peaks;
    }
    for s in &track.sections {
        if !matches!(s.label, S::Drop | S::Chorus) {
            continue;
        }
        if let Some(last) = peaks.last_mut() {
            if (s.start_sec - last.1).abs() < 0.1 {
                last.1 = s.end_sec;
                continue;
            }
        }
        peaks.push((s.start_sec, s.end_sec));
    }
    peaks.retain(|&(start, end)| end - start >= 3.5 * local_bar(track, start));
    peaks
}

fn local_bar(track: &TrackAnalysis, sec: f32) -> f32 {
    track
        .bars
        .iter()
        .find(|b| b.start_sec <= sec + 0.01 && b.end_sec > sec + 0.01)
        .map(|b| b.end_sec - b.start_sec)
        .filter(|d| *d >= 0.5)
        .unwrap_or(track.tempo.meter_num.max(1) as f32 * 60. / track.tempo.global_bpm.max(20.))
}

/// A short, measured retreat between sustained peaks is a suspension. Length
/// alone cannot distinguish one from a usable short breakdown. Compare its
/// source duration with both surrounding passages, independently of half/double
/// tempo interpretation, then require level, bass and attack retreat + return.
pub fn drop_suspension(track: &TrackAnalysis, sec: f32) -> Option<(f32, f32)> {
    peak_ranges(track).windows(2).find_map(|pair| {
        let (start, end) = (pair[0].1, pair[1].0);
        (end > start + 0.1
            && (end - start) * 2. <= (pair[0].1 - pair[0].0).min(pair[1].1 - pair[1].0)
            && sec >= start - 0.05
            && sec < end - 0.05
            // A recovery with its own supported phrase is an independent
            // passage, even between much longer peaks. Do not consume it just
            // because the whole interval is quieter than those peaks.
            && !track.phrase_boundaries.iter().any(|p| {
                p.confidence >= 0.5 && p.time_sec > start + 0.08 && p.time_sec < end - 0.08
            })
            && track
                .sections
                .iter()
                .filter(|s| s.start_sec < end && s.end_sec > start)
                .all(|s| {
                    matches!(
                        s.label,
                        S::Break | S::Breakdown | S::BuildUp | S::Silence | S::Unknown
                    )
                })
            && measured_retreat(track, start, end))
        .then_some((start, end))
    })
}

struct Activity {
    rms: f32,
    low_db: f32,
    attacks: f32,
}

fn activity(track: &TrackAnalysis, start: f32, end: f32) -> Option<Activity> {
    let (mut seconds, mut power, mut bass, mut attacks) = (0., 0., 0., 0.);
    for b in &track.bars {
        let w = (end.min(b.end_sec) - start.max(b.start_sec)).max(0.);
        if w == 0. {
            continue;
        }
        if ![b.rms, b.low_db, b.onset_density]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        seconds += w;
        power += b.rms * b.rms * w;
        bass += 10f32.powf(b.low_db / 10.) * w;
        attacks += b.onset_density * w;
    }
    (seconds > 0. && seconds >= (end - start) * 0.95).then(|| Activity {
        rms: (power / seconds).sqrt(),
        low_db: 10. * (bass / seconds).max(f32::MIN_POSITIVE).log10(),
        attacks: attacks / seconds,
    })
}

fn measured_retreat(track: &TrackAnalysis, start: f32, end: f32) -> bool {
    let width = end - start;
    let (Some(before), Some(gap), Some(after)) = (
        activity(track, start - width, start),
        activity(track, start, end),
        activity(track, end, end + width),
    ) else {
        return false;
    };
    // These are contrast thresholds, not genre/tempo rules: at least 3 dB
    // overall retreat, 6 dB bass retreat and a third fewer attacks on BOTH
    // sides. A changed section label or a single bass mute is insufficient.
    gap.rms <= before.rms.min(after.rms) * std::f32::consts::FRAC_1_SQRT_2
        && gap.low_db <= before.low_db.min(after.low_db) - 6.
        && before.attacks > 0.
        && after.attacks > 0.
        && gap.attacks <= before.attacks.min(after.attacks) * (2. / 3.)
        && after.rms >= before.rms * 0.5
        && after.low_db >= before.low_db - 6.
}

#[cfg(test)]
mod tests;

/// The peaks a play-through is held to, each one played whole. Adjacent
/// drop and chorus sections form one run, and a short run stays whole: a
/// section boundary inside it is a variation, not an exit. A run longer than
/// 32 bars is a sequence of complete sections (a sung track can stay at full
/// energy for minutes), so it splits on the boundaries the analysis measured,
/// keeping every part at least eight bars long.
pub fn peak_units(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    let bar = track.tempo.meter_num.max(1) as f32 * 60. / track.tempo.global_bpm.max(20.);
    let mut units = Vec::new();
    for (start, end) in peak_ranges(track) {
        if end - start <= 32. * bar + 0.05 {
            units.push((start, end));
            continue;
        }
        let mut from = start;
        for edge in track
            .sections
            .iter()
            .map(|s| s.start_sec)
            .filter(|&t| t > start + 0.05 && t < end - 0.05)
        {
            if edge - from >= 8. * bar - 0.05 && end - edge >= 8. * bar - 0.05 {
                units.push((from, edge));
                from = edge;
            }
        }
        units.push((from, end));
    }
    units
}

/// The track's full-weight peaks: at least eight bars long and within ~1.2 dB
/// of the loudest peak's median bar level. A lighter first chorus is not the
/// moment the track is waiting for.
pub fn major_peaks(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    let peaks = peak_units(track);
    let median = |p: &(f32, f32)| {
        let mut rms: Vec<_> = track
            .bars
            .iter()
            .filter(|b| b.start_sec >= p.0 - 0.01 && b.end_sec <= p.1 + 0.01)
            .map(|b| b.rms)
            .collect();
        rms.sort_by(f32::total_cmp);
        rms.get(rms.len() / 2).copied().unwrap_or(0.)
    };
    let top = peaks.iter().map(&median).fold(0., f32::max);
    let bar = 240. / track.tempo.global_bpm.max(20.);
    let major: Vec<_> = peaks
        .iter()
        .copied()
        .filter(|p| median(p) >= top * 0.87 && p.1 - p.0 >= 7.5 * bar)
        .collect();
    if major.is_empty() { peaks } else { major }
}

/// The peak a play-through owes: the first major peak still ahead of the
/// entry that is the track's main drop, at least 80% as long as the longest
/// one left. A shorter first drop is a preview; leaving after it skips the
/// moment the track builds towards. Peaks are compared as whole runs of drop
/// and chorus, so a long run split into sections still counts as one drop.
pub fn main_peak(track: &TrackAnalysis, entry: f32) -> Option<(f32, f32)> {
    let ahead: Vec<_> = major_peaks(track)
        .into_iter()
        .filter(|p| p.1 > entry + 0.1)
        .collect();
    let run = |p: &(f32, f32)| {
        peak_ranges(track)
            .into_iter()
            .find(|r| r.0 <= p.0 + 0.05 && r.1 >= p.1 - 0.05)
            .unwrap_or(*p)
    };
    let longest = ahead
        .iter()
        .map(|p| run(p))
        .map(|r| r.1 - r.0)
        .fold(0., f32::max);
    let first = ahead.iter().find(|p| {
        let r = run(p);
        r.1 - r.0 >= 0.8 * longest - 0.05
    })?;
    // The run is owed through its last section that is still a major peak.
    let r = run(first);
    ahead
        .into_iter()
        .filter(|p| p.0 >= r.0 - 0.05 && p.1 <= r.1 + 0.05)
        .last()
}
