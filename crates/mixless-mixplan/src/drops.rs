//! Prefer one meaningful peak per play-through; later exits remain fallbacks.
use mixless_protocol::{SectionLabel as S, TrackAnalysis};

/// A section boundary inside one continuous drop is a variation, not an exit.
/// A drop shorter than a fill is not a peak at all.
pub(super) fn peaks(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    mixless_protocol::peak_ranges(track)
}

/// The track's full-weight peaks: at least eight bars long and within ~1.2 dB
/// of the loudest peak's median bar level. A lighter first chorus is not the
/// moment the track is waiting for.
pub(super) fn major_peaks(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    let peaks = peaks(track);
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
    if major.is_empty() {
        peaks
    } else {
        major
    }
}

/// The next drop's start at or after `sec` — where the current build resolves.
pub(super) fn next_peak_start(track: &TrackAnalysis, sec: f32) -> Option<f32> {
    peaks(track).iter().map(|p| p.0).find(|&p| p >= sec - 0.05)
}

/// Preserve an entered peak's resolution. Starting a complementary layer
/// during it is allowed; its audible energy is handled by the arrangement.
pub(super) fn allows(track: &TrackAnalysis, start: f32, end: f32, entry: f32, cut: bool) -> bool {
    let peaks = peaks(track);
    let first = peaks
        .iter()
        .find(|p| p.0 + 0.1 >= entry)
        .or_else(|| peaks.iter().find(|p| p.1 > entry + 0.1));
    if first.is_some_and(|p| end + 0.05 < p.1 || (!cut && start < p.0 - 0.05)) {
        return false;
    }
    !peaks.iter().any(|&(a, b)| end > a + 0.05 && end < b - 0.05)
}

pub(super) fn recovery(label: S) -> bool {
    matches!(label, S::Break | S::Breakdown | S::BuildUp | S::Outro)
}

pub(super) fn entry_allowed(track: &TrackAnalysis, entry: f32) -> bool {
    !peaks(track)
        .iter()
        .any(|&(start, end)| entry > start + 0.05 && entry < end - 0.05)
}

pub(super) fn penalty(track: &TrackAnalysis, exit: f32, entry: f32) -> f32 {
    let peaks: Vec<(f32, f32)> = major_peaks(track)
        .into_iter()
        .filter(|p| p.1 > entry + 1.)
        .collect();
    let base = match peaks.first() {
        None => 0.,
        Some(first) if exit + 0.1 < first.1 => 0.22,
        _ => match peaks.get(1) {
            Some(second) if exit > second.0 + 0.1 => {
                let heard = ((exit - second.0) / (second.1 - second.0)).clamp(0., 1.);
                0.16 + heard * 0.16
                    + peaks.iter().skip(2).filter(|p| exit > p.0).count() as f32 * 0.08
            }
            _ => 0.,
        },
    };
    // Leaving in the first stretch of a track discards its main body even
    // when no peak is being cut short.
    base + (0.4 - exit / track.duration_sec).max(0.) * 0.5
}
