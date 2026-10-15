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
    // A long peak has resolved once a phrase line inside it is reached.
    if first.is_some_and(|&p| {
        (end + 0.05 < p.1 && !phrase_inside(track, p, end)) || (!cut && start < p.0 - 0.05)
    }) {
        return false;
    }
    !peaks
        .iter()
        .any(|&(a, b)| end > a + 0.05 && end < b - 0.05 && !phrase_inside(track, (a, b), end))
}

pub(super) fn recovery(label: S) -> bool {
    matches!(label, S::Break | S::Breakdown | S::BuildUp | S::Outro)
}

pub(super) fn entry_allowed(track: &TrackAnalysis, entry: f32) -> bool {
    !peaks(track).iter().any(|&(start, end)| {
        entry > start + 0.05 && entry < end - 0.05 && !phrase_inside(track, (start, end), entry)
    })
}

/// Dense tracks rarely leave the floor: drop and chorus sections merge into a
/// single peak lasting minutes. Past 32 bars it is a run of phrases, and DJs
/// enter or leave it on a 16-bar line at least 16 bars from either edge.
fn phrase_inside(track: &TrackAnalysis, (start, end): (f32, f32), sec: f32) -> bool {
    let grid = crate::grid::Grid(track);
    let meter = grid.meter();
    let (first, last, at) = (grid.beat(start), grid.beat(end), grid.beat(sec));
    let bars = (at - first) / meter;
    last - first >= 32. * meter
        && bars >= 16. - 0.01
        && (last - at) / meter >= 16. - 0.01
        && (bars / 16. - (bars / 16.).round()).abs() * 16. < 0.05
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

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::Section;

    fn with_peak(bars: u32, peak: (u32, u32)) -> TrackAnalysis {
        let mut t = crate::tests::track(1, 174., "8A", S::Intro, bars, 0.9, 0.2);
        let bar = 240. / 174.;
        let edge = |b: u32| b as f32 * bar;
        t.sections = vec![
            Section {
                start_sec: 0.,
                end_sec: edge(peak.0),
                label: S::Intro,
            },
            Section {
                start_sec: edge(peak.0),
                end_sec: edge(peak.1),
                label: S::Drop,
            },
            Section {
                start_sec: edge(peak.1),
                end_sec: edge(bars),
                label: S::Outro,
            },
        ];
        t
    }

    #[test]
    fn a_long_peak_opens_on_its_inner_sixteen_bar_lines_only() {
        let bar = 240. / 174.;
        let long = with_peak(112, (8, 88));
        assert!(entry_allowed(&long, 8. * bar), "the peak's own start");
        assert!(entry_allowed(&long, 24. * bar));
        assert!(entry_allowed(&long, 56. * bar));
        assert!(!entry_allowed(&long, 20. * bar), "not a 16-bar line");
        let closing = with_peak(112, (8, 80));
        assert!(
            !entry_allowed(&closing, 72. * bar),
            "a line 8 bars from the end"
        );
        assert!(allows(&long, 40. * bar, 56. * bar, 0., false));
        assert!(!allows(&long, 36. * bar, 52. * bar, 0., false));
        let short = with_peak(64, (8, 32));
        assert!(
            !entry_allowed(&short, 24. * bar),
            "a 24-bar drop stays whole"
        );
    }
}
