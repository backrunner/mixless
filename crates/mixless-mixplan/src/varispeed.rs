//! Key lock off for the incoming deck. A record played faster without key
//! lock also sounds higher: 5.95% is one semitone. When the speed a held
//! overlap needs is itself the transposition the keys need, the deck plays
//! unlocked: nothing is time-stretched or pitch-shifted, and the solo tempo
//! glide returns its pitch together with its tempo.
use mixless_protocol::{varispeed_semitones, TrackAnalysis};

/// Mistuning accepted between the speed's own pitch and the key that fits.
pub(crate) const TOLERANCE: f32 = 0.1;
/// Pitch the speed may wander over the overlap when B's own tempo moves.
const DRIFT: f32 = 0.05;

/// The sounding transposition when B should play unlocked through an overlap
/// that holds it at `rates`, or `None` to keep key lock on. `locked` is the
/// ordinary key decision for the same windows: score, shift and reliability.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fit(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    ra: (f32, f32),
    rb: (f32, f32),
    pa: f32,
    pb: f32,
    rates: &[f32],
    locked: (f32, f32, bool),
) -> Option<f32> {
    let (key, shift, reliable) = locked;
    // An explicit tuning keeps the stretcher engaged, so nothing is gained.
    if !reliable || pb.abs() > 0.005 || rates.iter().any(|r| !r.is_finite() || *r <= 0.) {
        return None;
    }
    let low = rates.iter().copied().reduce(f32::min)?;
    let high = rates.iter().copied().reduce(f32::max)?;
    if varispeed_semitones(high / low) > DRIFT {
        return None;
    }
    let pitch = varispeed_semitones((low * high).sqrt());
    // Under half a semitone the speed is fine tuning, which a locked deck
    // matches exactly. Beyond the limit it is not an automatic transposition.
    if pitch.abs() < 0.5 || pitch.abs() > crate::score::MAX_KEY_SHIFT + TOLERANCE {
        return None;
    }
    // A record that fits as recorded is not transposed, and a smaller locked
    // shift remains the smaller change.
    if (shift == 0. && key >= 0.85)
        || (shift != 0. && key > 0. && shift.abs() < pitch.abs().round() - 0.5)
    {
        return None;
    }
    let (score, residual, reliable) = crate::score::overlap_key_match(a, b, ra, rb, pa, pitch)
        .unwrap_or_else(|| crate::score::window_key_match(a, b, ra.1, rb.0, pa, pitch));
    (reliable && score > 0. && residual.abs() <= TOLERANCE).then_some(pitch)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::SectionLabel as S;

    const SEMITONE: f32 = 1.059_463_1;

    fn pair(key_b: &str) -> (TrackAnalysis, TrackAnalysis) {
        (
            crate::tests::track(1, 120., "8A", S::Outro, 64, 0.9, 0.2),
            crate::tests::track(2, 120., key_b, S::Intro, 64, 0.9, 0.2),
        )
    }

    fn decide(key_b: &str, rates: &[f32], pb: f32) -> Option<f32> {
        let (a, b) = pair(key_b);
        let locked = crate::score::window_key_match(&a, &b, 128., 0., 0., pb);
        fit(&a, &b, (96., 128.), (0., 32.), 0., pb, rates, locked)
    }

    #[test]
    fn the_speed_is_used_as_the_transposition_only_when_it_is_the_one_the_keys_need() {
        // 1A a semitone up is 8A; 3A a semitone down is 8A.
        let up = decide("1A", &[SEMITONE; 3], 0.).unwrap();
        assert!((up - 1.).abs() < 1e-3);
        let down = decide("3A", &[1. / SEMITONE; 3], 0.).unwrap();
        assert!((down + 1.).abs() < 1e-3);
        // The right speed in the wrong direction, or for keys it does not fit.
        assert!(decide("1A", &[1. / SEMITONE; 3], 0.).is_none());
        assert!(decide("5A", &[SEMITONE; 3], 0.).is_none());
        // Ten cents either side is accepted, more is not.
        assert!(decide("1A", &[SEMITONE * 1.005; 3], 0.).is_some());
        assert!(decide("1A", &[SEMITONE * 0.995; 3], 0.).is_some());
        assert!(decide("1A", &[SEMITONE * 1.008; 3], 0.).is_none());
        assert!(decide("1A", &[1.04; 3], 0.).is_none());
        // Two semitones is the limit of automatic transposition.
        assert!(decide("6A", &[SEMITONE * SEMITONE; 3], 0.).is_some());
        assert!(decide("1A", &[SEMITONE * SEMITONE * SEMITONE; 3], 0.).is_none());
    }

    #[test]
    fn a_record_that_fits_as_recorded_or_with_a_smaller_shift_stays_locked() {
        // 8A and 9A are neighbours: no transposition, whatever the speed.
        assert!(decide("9A", &[SEMITONE; 3], 0.).is_none());
        assert!(decide("8A", &[SEMITONE; 3], 0.).is_none());
        // A speed worth two semitones loses to a one-semitone locked shift.
        let (a, b) = pair("1A");
        let locked = crate::score::window_key_match(&a, &b, 128., 0., 0., 0.);
        assert_eq!(locked.1, 1.);
        assert!(fit(
            &a,
            &b,
            (96., 128.),
            (0., 32.),
            0.,
            0.,
            &[SEMITONE * SEMITONE; 3],
            locked
        )
        .is_none());
    }

    #[test]
    fn explicit_tuning_wandering_tempo_and_weak_keys_stay_locked() {
        assert!(decide("1A", &[SEMITONE; 3], 0.25).is_none());
        assert!(decide("1A", &[SEMITONE, SEMITONE * 1.004, SEMITONE], 0.).is_none());
        assert!(decide("1A", &[SEMITONE, SEMITONE * 1.002, SEMITONE], 0.).is_some());
        assert!(decide("1A", &[], 0.).is_none());
        assert!(decide("1A", &[SEMITONE, f32::NAN], 0.).is_none());
        let (a, mut b) = pair("1A");
        b.key_confidence = 0.1;
        for region in &mut b.mix_regions {
            region.key_confidence = 0.1;
        }
        let locked = crate::score::window_key_match(&a, &b, 128., 0., 0., 0.);
        assert!(!locked.2);
        assert!(fit(
            &a,
            &b,
            (96., 128.),
            (0., 32.),
            0.,
            0.,
            &[SEMITONE; 3],
            locked
        )
        .is_none());
    }
}
