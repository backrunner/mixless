//! Prefer one meaningful peak per play-through; later exits remain fallbacks.
use mixless_protocol::{SectionLabel as S, TrackAnalysis};

/// The peaks a play-through is held to, each one played whole.
pub(super) fn peaks(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    mixless_protocol::peak_units(track)
}

/// Continuous runs of drop and chorus sections; a drop shorter than a fill
/// is not a peak at all.
pub(super) fn runs(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    mixless_protocol::peak_ranges(track)
}

/// The track's full-weight peaks.
pub(super) fn major_peaks(track: &TrackAnalysis) -> Vec<(f32, f32)> {
    mixless_protocol::major_peaks(track)
}

/// The next drop's start at or after `sec` — where the current build
/// resolves. A new section inside a running peak resolves nothing.
pub(super) fn next_peak_start(track: &TrackAnalysis, sec: f32) -> Option<f32> {
    runs(track).iter().map(|p| p.0).find(|&p| p >= sec - 0.05)
}

/// The peak a play-through owes: the track's main drop ahead of `entry`.
pub(super) fn owed_peak(track: &TrackAnalysis, entry: f32) -> Option<(f32, f32)> {
    mixless_protocol::main_peak(track, entry)
}

/// A peak plays whole. The outgoing track never leaves inside a peak, never
/// before the major peak it owes has finished, and an overlap only begins
/// once that peak has arrived; a cut may leave from its build instead.
pub(super) fn allows(track: &TrackAnalysis, start: f32, end: f32, entry: f32, cut: bool) -> bool {
    if owed_peak(track, entry).is_some_and(|p| end + 0.05 < p.1 || (!cut && start < p.0 - 0.05)) {
        return false;
    }
    !inside_peak(track, end)
}

/// Strictly inside a peak, away from both of its edges.
pub(super) fn inside_peak(track: &TrackAnalysis, sec: f32) -> bool {
    peaks(track)
        .iter()
        .any(|&(start, end)| sec > start + 0.05 && sec < end - 0.05)
}

pub(super) fn recovery(label: S) -> bool {
    matches!(label, S::Break | S::Breakdown | S::BuildUp | S::Outro)
}

/// The incoming track starts outside its peaks, and with at least one major
/// peak still ahead, so the play-through reaches what the track is about.
pub(super) fn entry_allowed(track: &TrackAnalysis, entry: f32) -> bool {
    let major = major_peaks(track);
    !inside_peak(track, entry) && (major.is_empty() || major.iter().any(|p| p.0 >= entry - 0.05))
}

/// A track that opens on its peak has no build to skip: a phrase line inside
/// that opening run is no worse an entry than the top of the track.
pub(super) fn opening_peak(track: &TrackAnalysis, entry: f32) -> bool {
    let g = crate::grid::Grid(track);
    let opening = g.sec(16. * g.meter());
    peaks(track)
        .first()
        .is_some_and(|p| p.0 <= opening && entry > p.0 && entry < p.1)
}

/// Major peaks the entry skips; each one is part of the track never heard.
pub(super) fn skipped(track: &TrackAnalysis, entry: f32) -> usize {
    major_peaks(track)
        .iter()
        .filter(|p| p.0 < entry - 0.05)
        .count()
}

pub(super) fn penalty(track: &TrackAnalysis, exit: f32, entry: f32) -> f32 {
    let peaks: Vec<(f32, f32)> = major_peaks(track)
        .into_iter()
        .filter(|p| p.1 > entry + 1.)
        .collect();
    // The main drop is what a play-through owes; once it has played, the
    // first clean exit after it is the natural one, and each later peak
    // entered before leaving costs more.
    let owed = owed_peak(track, entry);
    let base = match owed {
        None => 0.,
        Some(main) if exit + 0.1 < main.1 => 0.22,
        Some(main) => {
            let later: Vec<_> = peaks.iter().filter(|p| p.0 >= main.1 - 0.05).collect();
            match later.first() {
                Some(next) if exit > next.0 + 0.1 => {
                    let heard = ((exit - next.0) / (next.1 - next.0)).clamp(0., 1.);
                    0.16 + heard * 0.16
                        + later.iter().skip(1).filter(|p| exit > p.0).count() as f32 * 0.08
                }
                // After the owed peak has played, penalise leaving before
                // the next major peak starts — a second drop run is worth
                // hearing just as much as the first one.
                Some(next) if exit + 0.1 < next.0 => {
                    // Scale by how much of the remaining track is that peak.
                    let remaining = (track.duration_sec - main.1).max(1.);
                    let weight = ((next.1 - next.0) / remaining).clamp(0., 1.);
                    0.08 * weight
                }
                _ => 0.,
            }
        }
    };
    // Leaving in the first stretch of a track discards its main body even
    // when no peak is being cut short.
    base + (0.4 - exit / track.duration_sec).max(0.) * 0.5
}

/// A play-through shorter than a DJ would give the track: up to 0.3 when
/// leaving straight after the entry, nothing once about 90 s (or 45% of a
/// short track) has been heard.
pub(super) fn brief(track: &TrackAnalysis, exit: f32, entry: f32) -> f32 {
    let want = (0.45 * track.duration_sec).min(90.);
    ((want - (exit - entry)) / want.max(1.)).clamp(0., 1.) * 0.3
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
    fn a_peak_is_entered_and_left_only_at_its_edges() {
        let bar = 240. / 174.;
        let long = with_peak(112, (8, 88));
        assert!(entry_allowed(&long, 8. * bar), "the peak's own start");
        assert!(entry_allowed(&long, 0.), "the intro");
        assert!(!entry_allowed(&long, 24. * bar), "not on a line inside it");
        assert!(!entry_allowed(&long, 96. * bar), "no peak left to hear");
        assert!(!allows(&long, 40. * bar, 56. * bar, 0., false));
        // The handoff inside such an overlap is held to the peak's end
        // separately; the window itself may open in the last phrase.
        assert!(allows(&long, 72. * bar, 88. * bar, 0., false));
        assert!(
            allows(&long, 80. * bar, 96. * bar, 0., false),
            "overlap opens in its last phrase"
        );
        assert!(
            !allows(&long, 0., 8. * bar, 0., true),
            "a cut before the owed peak"
        );
        assert!(
            allows(&long, 88. * bar, 88. * bar, 0., true),
            "a cut on its end"
        );
    }

    #[test]
    fn a_long_run_of_sections_is_held_section_by_section() {
        let bar = 240. / 174.;
        let mut t = with_peak(80, (8, 64));
        let at = |b: f32| b * bar;
        t.sections = vec![
            Section {
                start_sec: 0.,
                end_sec: at(8.),
                label: S::Intro,
            },
            Section {
                start_sec: at(8.),
                end_sec: at(24.),
                label: S::Chorus,
            },
            Section {
                start_sec: at(24.),
                end_sec: at(28.),
                label: S::Drop,
            },
            Section {
                start_sec: at(28.),
                end_sec: at(64.),
                label: S::Drop,
            },
            Section {
                start_sec: at(64.),
                end_sec: at(80.),
                label: S::Outro,
            },
        ];
        let units = peaks(&t);
        assert_eq!(units.len(), 2, "{units:?}");
        assert!(
            (units[0].1 - at(24.)).abs() < 0.01,
            "a four-bar section is not a unit"
        );
        // The 40-bar drop is the main one: the 16-bar chorus is a preview.
        assert!(
            !allows(&t, at(16.), at(24.), 0., true),
            "the main drop is owed"
        );
        assert!(allows(&t, at(64.), at(64.), 0., true));
        assert!(!allows(&t, at(32.), at(40.), 0., false), "inside the drop");
        // Leaving between units is not leaving into a new build.
        assert_eq!(next_peak_start(&t, at(24.)), None);
        let short = with_peak(48, (8, 32));
        assert_eq!(peaks(&short).len(), 1);
    }
}
