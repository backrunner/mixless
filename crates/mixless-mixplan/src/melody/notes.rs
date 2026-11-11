use mixless_protocol::{instrument_boundaries, InstrumentBoundaryKind as Kind, TrackAnalysis};

pub(super) fn windows(t: &TrackAnalysis) -> Vec<(f32, f32)> {
    instrument_boundaries(t)
        .into_iter()
        .filter_map(|b| match b.kind {
            // Use the measured gap: the completed group's last note stays audible
            // through its release; the next group starts after the lane has moved.
            Kind::Rest => Some((b.release_sec, b.next_attack_sec)),
            // Each active pitch/time voice corroborates this complete cycle.
            // Fade through that cycle, not through an arbitrary note gap.
            Kind::Motif => b
                .period_sec
                .map(|period| (b.next_attack_sec - period, b.next_attack_sec)),
        })
        .collect()
}

pub(super) fn supports(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let grid = crate::grid::Grid(t);
    // The gain probe reports 1–90% crossings rather than the mathematical
    // endpoints of a fade. Match the entire audible move to one fitted window.
    windows(t).into_iter().any(|(lo, hi)| {
        let width = hi - lo;
        let tolerance = width * 0.15;
        start >= lo - 0.001
            && start <= lo + tolerance
            && end >= hi - tolerance
            && end <= hi + 0.001
            && (grid.beat(end) - grid.beat(start)).abs() >= grid.meter() * 0.75
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{Polyline, SectionLabel, StemAnalysis, StemFrame, StemKind, StemNote};
    #[test]
    fn measured_note_rest_moves_a_layer_without_inventing_a_whole_track_exit() {
        let mut t = crate::tests::track(1, 120., "8A", SectionLabel::Outro, 16, 0.8, 0.2);
        t.stems = Some(StemAnalysis {
            version: 2,
            separator_sha256: String::new(),
            notes_sha256: String::new(),
            duration_sec: t.duration_sec,
            residual_rms: 0.,
            frames: (0..(t.duration_sec * 20.) as usize)
                .map(|i| {
                    let sec = i as f32 * 0.05;
                    let quiet = (8. ..10.).contains(&sec);
                    StemFrame {
                        start_sec: sec,
                        end_sec: (i + 1) as f32 * 0.05,
                        rms: [0.1, 0.1, if quiet { 0.01 } else { 0.3 }],
                        band_db: [[-20.; 3]; 3],
                        onset: [0.; 3],
                        vocal_activity: 0.,
                        drum_low_onset: 0.,
                        note_chroma: [0.; 12],
                    }
                })
                .collect(),
            notes: [(4., 5., 60), (5.1, 6., 64), (6.1, 8., 67), (10., 12., 65)]
                .into_iter()
                .map(|(start_sec, end_sec, midi)| StemNote {
                    stem: StemKind::Instruments,
                    start_sec,
                    end_sec,
                    midi,
                    confidence: 0.9,
                })
                .collect(),
        });
        assert!(windows(&t).contains(&(8., 10.)));
        assert!(supports(&t, 8.2, 9.98));
        assert!(
            !supports(&t, 7.9, 9.98),
            "the fade cannot begin inside the held final note"
        );
        assert!(
            !supports(&t, 8.2, 10.1),
            "the fade cannot remove the next attack"
        );
        assert!(!crate::phrasing::exit_boundary(&t, 10.));
        let source = Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        };
        assert!(super::super::note_windows(&t, &source, 16., (4., 5.), true).contains(&(4., 5.)));
        // Neither silence in the note transcript alone nor a long note that
        // crosses the proposed boundary creates a release window.
        t.stems.as_mut().unwrap().notes[2].end_sec = 11.;
        assert!(windows(&t).is_empty());
    }
}
