//! Independent attack-time check for passages whose broad-band autocorrelation
//! is diluted by syncopation. Never grants confidence from a generated grid alone.
use mixless_protocol::TrackAnalysis;

pub(crate) fn supports_grid(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let grid = crate::grid::Grid(t);
    let period = 60. / grid.bpm(grid.beat(start));
    let first = t.moments.partition_point(|m| m.end_sec <= start);
    let last = t.moments.partition_point(|m| m.start_sec < end);
    let mut evidence = [0f32; 3];
    let mut hits = 0;
    let mut covered = [false; 4];
    let mut error = 0.;
    let mut weight = 0.;
    let mut previous = -1.;
    for m in &t.moments[first..last] {
        if m.low_onset < 0.45 || m.attack_sec - previous < 0.09 {
            continue;
        }
        previous = m.attack_sec;
        // MusicalMoment stores the low-band rise, whose analysis window
        // peaks about 30 ms after the broad-band attack used by the grid.
        let beat = grid.beat(m.attack_sec - 0.03);
        let offset = (beat - beat.round()) * period;
        for (i, shift) in [0., -0.25, 0.25].into_iter().enumerate() {
            let distance =
                (offset - shift * period + period * 0.5).rem_euclid(period) - period * 0.5;
            evidence[i] += m.low_onset * (1. - distance.abs() / 0.05).max(0.);
        }
        if offset.abs() <= 0.045 {
            hits += 1;
            error += offset.abs() * m.low_onset;
            weight += m.low_onset;
            covered[(((m.attack_sec - start) / (end - start) * 4.) as usize).min(3)] = true;
        }
    }
    hits >= 4
        && covered.iter().filter(|x| **x).count() >= 3
        && error / weight.max(1e-6) < 0.03
        && evidence[0] > 1.4 * evidence[1].max(evidence[2]).max(0.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn low_band_measurement_lag_is_not_a_beat_offset() {
        let mut t = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Intro,
            32,
            0.5,
            0.1,
        );
        for shift in [0., 0.125] {
            t.moments = (0..24)
                .map(|i| {
                    let sec = i as f32 * 0.5 + 0.03 + shift;
                    mixless_protocol::MusicalMoment {
                        start_sec: sec,
                        end_sec: sec + 0.05,
                        rms: 0.2,
                        band_db: [-20.; 3],
                        chroma: [0.; 12],
                        onset: 1.,
                        attack_sec: sec,
                        low_onset: 1.,
                        sustain: 0.,
                        pitch_midi: None,
                        vocal_confidence: Some(0.),
                    }
                })
                .collect();
            assert_eq!(supports_grid(&t, 0., 12.), shift == 0.);
        }
        t.moments.truncate(2);
        assert!(!supports_grid(&t, 0., 12.));
    }
}
