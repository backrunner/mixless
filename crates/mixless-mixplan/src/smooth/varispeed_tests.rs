//! Key lock off for the incoming deck, through a real four-bar held overlap.
use super::recovery_tests::{context, recovery_track};
use super::*;
use mixless_protocol::{varispeed_semitones, SectionLabel as L};

const SEMITONE: f32 = 1.059_463_1;

fn options() -> PlannerOptions {
    PlannerOptions {
        harmonic_key_shift: true,
        ..Default::default()
    }
}

/// The outgoing record is in 8A. The overlap is too short to glide, so the
/// incoming record is held at the outgoing tempo: `ratio` times its own.
fn try_held(
    ratio: f32,
    key: &str,
    options: &PlannerOptions,
) -> (TrackAnalysis, TrackAnalysis, Option<MixPlan>) {
    let a = recovery_track(120. * ratio);
    let b = crate::tests::track(2, 120., key, L::Intro, 64, 0.8, 0.2);
    let p = pair(
        &context(&a, &b),
        options,
        128.,
        0.,
        TransitionMode::BeatBlend,
        4,
        -1.,
    );
    (a, b, p)
}

fn held(
    ratio: f32,
    key: &str,
    options: &PlannerOptions,
) -> (TrackAnalysis, TrackAnalysis, MixPlan) {
    let (a, b, p) = try_held(ratio, key, options);
    (
        a,
        b,
        p.unwrap_or_else(|| panic!("no held overlap for {key} at {ratio}")),
    )
}

#[test]
fn a_semitone_of_speed_replaces_stretching_and_shifting_when_the_keys_want_it() {
    // 1A played 5.95% faster sounds in 8A.
    let (_, _, p) = held(SEMITONE, "1A", &options());
    assert!(p.incoming_varispeed);
    assert!(p.lanes.pitch_b.nodes.iter().all(|(_, pitch)| *pitch == 0.));
    assert!((p.incoming_offset_end.rate - SEMITONE).abs() < 0.001);
    assert!((p.incoming_offset_end.pitch_semitones - 1.).abs() < 0.02);
    assert!((p.incoming_pitch_at(2.) - 1.).abs() < 0.02);
    // The outgoing deck is never unlocked or retuned.
    assert!(p.lanes.pitch_a.nodes.iter().all(|(_, pitch)| *pitch == 0.));
    // The same pair locked would have been shifted by the same semitone.
    let (a, b, _) = held(SEMITONE, "1A", &options());
    assert_eq!(
        crate::score::window_key_match(&a, &b, 64., 0., 0., 0.).1,
        1.
    );
}

#[test]
fn the_deck_stays_locked_when_speed_and_key_do_not_agree() {
    // Fits as recorded: stretched to the tempo, never transposed.
    for key in ["8A", "9A"] {
        let (_, _, p) = held(SEMITONE, key, &options());
        assert!(!p.incoming_varispeed, "{key}");
        assert_eq!(p.incoming_offset_end.pitch_semitones, 0.);
    }
    // 3A needs a semitone down while the speed goes up.
    let (_, _, p) = held(SEMITONE, "3A", &options());
    assert!(!p.incoming_varispeed);
    assert_eq!(p.lanes.pitch_b.nodes, vec![(0., -1.)]);
    // 6.7% is 12 cents past a semitone: locked, with an exact shift.
    let (_, _, p) = held(128. / 120., "1A", &options());
    assert!(!p.incoming_varispeed);
    assert_eq!(p.lanes.pitch_b.nodes, vec![(0., 1.)]);
    assert_eq!(p.incoming_offset_end.pitch_semitones, 1.);
    // Automatic transposition switched off covers this one too: two full
    // tracks in clashing keys do not blend at all.
    let (_, _, p) = try_held(SEMITONE, "1A", &PlannerOptions::default());
    assert!(p.is_none());
}

#[test]
fn the_solo_glide_returns_tempo_and_pitch_together() {
    let (a, b, mut p) = held(SEMITONE, "1A", &options());
    let planner = crate::Planner::with_options(options());
    let ctx = context(&a, &b);
    let overlap = p.clone();
    // A faster next track would keep part of a locked deck's offset. An
    // unlocked deck would keep a fraction of a semitone with it, so it does not.
    let next = crate::tests::track(3, 150., "8A", L::Intro, 64, 0.8, 0.2);
    crate::tempo_recovery::arrange(&planner, &ctx, Some(&next), &[], &mut p);
    assert!(p.incoming_varispeed);
    assert_eq!(p.stages.last().unwrap().label, "Restore tempo");
    assert!((p.incoming_offset_end.rate - 1.).abs() < 1e-4);
    assert!(p.incoming_offset_end.pitch_semitones.abs() < 0.002);
    let end = p.clock.nodes.last().unwrap().0;
    assert!(p.incoming_pitch_at(end).abs() < 0.002);
    // The pitch follows the speed all the way; the KEY control never moves.
    assert!(p.lanes.pitch_b.nodes.iter().all(|(_, pitch)| *pitch == 0.));
    let n = overlap.summary.as_ref().unwrap().length_bars as f32;
    for pair in p.lanes.rate_b.nodes.windows(2).filter(|w| w[0].0 >= n) {
        let seconds = p.clock.sample(pair[1].0) - p.clock.sample(pair[0].0);
        let cents = 100. * varispeed_semitones(pair[1].1 / pair[0].1).abs();
        // At most about six cents a second.
        assert!(cents <= 6.2 * seconds, "{cents} cents in {seconds} s");
    }
    // The separate pitch restoration has nothing left to do, and must not
    // move the KEY control of an unlocked deck even when a part remains.
    let settled = p.clone();
    crate::pitch_recovery::arrange(&planner, &ctx, None, &[], &[], &mut p);
    assert_eq!(p.lanes.pitch_b.nodes, settled.lanes.pitch_b.nodes);
    let mut partial = overlap;
    crate::pitch_recovery::arrange(&planner, &ctx, None, &[], &[], &mut partial);
    assert!(partial
        .lanes
        .pitch_b
        .nodes
        .iter()
        .all(|(_, pitch)| *pitch == 0.));
    assert!((partial.incoming_offset_end.pitch_semitones - 1.).abs() < 0.02);
}
