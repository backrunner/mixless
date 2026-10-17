use super::*;
use mixless_protocol::{
    Polyline, SectionLabel, StemAnalysis, StemFrame, StemKind, StemNote, TrackId,
};

pub(crate) fn with_stems(mut track: TrackAnalysis) -> TrackAnalysis {
    let frames = (0..(track.duration_sec / 0.05).ceil() as usize)
        .map(|i| StemFrame {
            start_sec: i as f32 * 0.05,
            end_sec: ((i + 1) as f32 * 0.05).min(track.duration_sec),
            rms: [0.1, 0.1, 0.2],
            band_db: [[-20.; 3]; 3],
            onset: [0.; 3],
            drum_low_onset: 0.,
            vocal_activity: 0.9,
            note_chroma: std::array::from_fn(|p| if p == 0 { 1. } else { 0. }),
        })
        .collect();
    track.stems = Some(StemAnalysis {
        version: 1,
        separator_sha256: "test".into(),
        notes_sha256: "test".into(),
        duration_sec: track.duration_sec,
        residual_rms: 0.,
        frames,
        notes: vec![],
    });
    track
}

#[test]
fn stem_envelopes_follow_evidence_restore_incoming_and_leave_cuts_untouched() {
    let a = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        32,
        0.8,
        0.2,
    ));
    let mut b = a.clone();
    b.track_id = TrackId(2);
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let mut plan = MixPlan {
        summary: Some(mixless_protocol::MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: mixless_protocol::StrategyId::PhraseBlend,
            score: 1.,
            used_fallback: false,
            length_bars: 16,
        }),
        clock: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        outgoing_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        incoming_source: Polyline {
            nodes: vec![(0., 0.), (16., 32.)],
        },
        ..Default::default()
    };
    plan.lanes.gain_a = Polyline {
        nodes: vec![(0., 0.), (16., -36.)],
    };
    plan.lanes.gain_b = Polyline {
        nodes: vec![(0., -36.), (16., 0.)],
    };
    crate::stem_mix::arrange(&ctx, &mut plan, false);
    let lanes = plan.stem_mix.as_ref().unwrap();
    assert!(lanes.incoming[0].sample(4.) < 0.5);
    assert!(lanes.outgoing[0].sample(12.) < 0.5);
    assert_eq!(lanes.incoming[0].sample(16.), 1.);
    assert!(
        lanes.incoming[2].nodes.iter().all(|(_, v)| *v == 1.),
        "Consonant instruments need no suppression"
    );
    let mut cut = plan.clone();
    cut.summary.as_mut().unwrap().length_bars = 0;
    cut.stem_mix = None;
    crate::stem_mix::arrange(&ctx, &mut cut, false);
    assert!(cut.stem_mix.is_none());
    let mut missing = b.clone();
    missing.stems = None;
    let ctx = PlanContext {
        incoming: &missing,
        ..ctx
    };
    plan.stem_mix = None;
    crate::stem_mix::arrange(&ctx, &mut plan, false);
    assert!(plan.stem_mix.is_none());
}
#[test]
fn a_sung_note_cannot_be_cut_at_an_inferred_bar_or_drop_marker() {
    let mut track = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.7,
        0.2,
    ));
    track.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: 3.,
        end_sec: 5.,
        midi: 60,
        confidence: 0.9,
    });
    assert!(!crate::continuity::cut_safe(&track, 4., true));
    assert!(!crate::vocals::safe_entry(&track, 4.));
    track.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 4.,
            label: SectionLabel::BuildUp,
        },
        mixless_protocol::Section {
            start_sec: 4.,
            end_sec: track.duration_sec,
            label: SectionLabel::Drop,
        },
    ];
    assert!(!crate::continuity::voice_cut_safe(&track, 4.));
    // The onset-at-boundary exemption requires the phrase to have released:
    // with the voice still singing, the new note is only a new syllable.
    track.stems.as_mut().unwrap().notes[0].start_sec = 4.;
    assert!(!crate::continuity::voice_cut_safe(&track, 4.));
    for frame in &mut track.stems.as_mut().unwrap().frames {
        if frame.start_sec >= 3.4 && frame.start_sec < 4.0 {
            frame.vocal_activity = 0.;
        }
    }
    track.stems.as_mut().unwrap().notes[0].end_sec = 3.4;
    assert!(crate::continuity::voice_cut_safe(&track, 4.));
    // A breath can release the voice, while a held instrument still disallows
    // a hard dry cut. A resolving build/drop may intentionally replace the riser.
    let stems = track.stems.as_mut().unwrap();
    stems.notes.clear();
    stems.notes.push(StemNote {
        stem: StemKind::Instruments,
        start_sec: 3.,
        end_sec: 5.,
        midi: 60,
        confidence: 0.9,
    });
    for frame in &mut stems.frames {
        frame.rms[0] = 0.;
        frame.vocal_activity = 0.;
    }
    assert!(crate::continuity::voice_cut_safe(&track, 4.));
    assert!(!crate::continuity::cut_safe(&track, 4., false));
}
#[test]
fn a_new_note_onset_cannot_disguise_a_still_singing_phrase_at_a_boundary() {
    let mut track = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.7,
        0.2,
    ));
    track.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 4.,
            label: SectionLabel::BuildUp,
        },
        mixless_protocol::Section {
            start_sec: 4.,
            end_sec: track.duration_sec,
            label: SectionLabel::Drop,
        },
    ];
    track.stems.as_mut().unwrap().notes = vec![
        StemNote {
            stem: StemKind::Vocals,
            start_sec: 1.5,
            end_sec: 3.9,
            midi: 60,
            confidence: 0.9,
        },
        StemNote {
            stem: StemKind::Vocals,
            start_sec: 4.0,
            end_sec: 6.0,
            midi: 62,
            confidence: 0.9,
        },
    ];
    // Every frame reports an active voice; the boundary note is a new syllable.
    assert!(!crate::continuity::voice_cut_safe(&track, 4.));
    assert!(!crate::continuity::voice_released(&track, 4.));
}

#[test]
fn a_boundary_note_onset_is_a_safe_cut_once_the_phrase_has_released() {
    let mut track = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.7,
        0.2,
    ));
    track.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 4.,
            label: SectionLabel::BuildUp,
        },
        mixless_protocol::Section {
            start_sec: 4.,
            end_sec: track.duration_sec,
            label: SectionLabel::Drop,
        },
    ];
    for frame in &mut track.stems.as_mut().unwrap().frames {
        if frame.start_sec >= 3.4 && frame.start_sec < 4.0 {
            frame.vocal_activity = 0.;
        }
    }
    track.stems.as_mut().unwrap().notes = vec![StemNote {
        stem: StemKind::Vocals,
        start_sec: 4.0,
        end_sec: 6.0,
        midi: 62,
        confidence: 0.9,
    }];
    // The cut itself is safe — it lands before the new note sounds — but the
    // voice continues past the boundary, so it is not a released exit for a
    // hard cut; the planner should blend there or pick another exit.
    assert!(crate::continuity::voice_cut_safe(&track, 4.));
    assert!(!crate::continuity::voice_released(&track, 4.));
}

#[test]
fn a_voice_that_stops_on_the_boundary_is_a_released_cut() {
    let mut track = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.7,
        0.2,
    ));
    track.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 4.,
            label: SectionLabel::BuildUp,
        },
        mixless_protocol::Section {
            start_sec: 4.,
            end_sec: track.duration_sec,
            label: SectionLabel::Drop,
        },
    ];
    // The vocal sings through the whole build and stops on the downbeat —
    // no quiet run-up required for a hard cut to land there.
    for frame in &mut track.stems.as_mut().unwrap().frames {
        frame.vocal_activity = if frame.start_sec < 4.0 { 0.9 } else { 0. };
    }
    track.stems.as_mut().unwrap().notes = vec![StemNote {
        stem: StemKind::Vocals,
        start_sec: 2.5,
        end_sec: 3.95,
        midi: 60,
        confidence: 0.9,
    }];
    assert!(crate::continuity::voice_cut_safe(&track, 4.));
    assert!(crate::continuity::voice_released(&track, 4.));
    // A note whose transcription overshoots the boundary slightly still
    // counts as released when nothing sounds after it.
    track.stems.as_mut().unwrap().notes[0].end_sec = 4.05;
    assert!(crate::continuity::voice_released(&track, 4.));
}

#[test]
fn the_planner_no_longer_dry_cuts_into_an_active_outgoing_vocal() {
    let mut a = with_stems(crate::tests::track(
        1,
        128.,
        "8A",
        SectionLabel::Drop,
        64,
        0.9,
        0.8,
    ));
    // A sung phrase spans bars 44..52 (82.5–97.5 s), crossing the measured
    // section boundary at 90 s. A new note begins exactly on that boundary.
    for frame in &mut a.stems.as_mut().unwrap().frames {
        frame.vocal_activity = if (82.5..97.5).contains(&frame.start_sec) {
            1.
        } else {
            0.
        };
    }
    a.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 90.,
            label: SectionLabel::Drop,
        },
        mixless_protocol::Section {
            start_sec: 90.,
            end_sec: a.duration_sec,
            label: SectionLabel::Outro,
        },
    ];
    a.stems.as_mut().unwrap().notes = vec![
        StemNote {
            stem: StemKind::Vocals,
            start_sec: 82.5,
            end_sec: 89.9,
            midi: 60,
            confidence: 0.9,
        },
        StemNote {
            stem: StemKind::Vocals,
            start_sec: 90.0,
            end_sec: 97.5,
            midi: 62,
            confidence: 0.9,
        },
    ];
    // Reliably incompatible key: blends are out, so a dry cut at 90 s would
    // have won before the hard vocal gate.
    let mut b = with_stems(crate::tests::track(
        2,
        128.,
        "4A",
        SectionLabel::Intro,
        64,
        0.8,
        0.9,
    ));
    for frame in &mut b.stems.as_mut().unwrap().frames {
        frame.vocal_activity = 0.;
    }
    let plan = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    if plan.failure_reason.is_none() {
        assert!(
            !(plan.t_out_a > 82.4 && plan.t_out_a < 97.6),
            "transition still exits inside the sung phrase: {:?} t_out_a={}",
            plan.summary,
            plan.t_out_a,
        );
    }
}

#[test]
fn stem_playback_enables_a_drum_layered_blend_across_incompatible_keys() {
    let mut a = with_stems(crate::tests::track(
        1,
        128.,
        "8A",
        SectionLabel::Drop,
        64,
        0.9,
        0.8,
    ));
    a.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 90.,
            label: SectionLabel::Drop,
        },
        mixless_protocol::Section {
            start_sec: 90.,
            end_sec: a.duration_sec,
            label: SectionLabel::Outro,
        },
    ];
    // The voice sings through the whole drop and takes a measured breath only
    // on the section boundary at 90 s, so it is the single place any exit may
    // land.
    for frame in &mut a.stems.as_mut().unwrap().frames {
        frame.vocal_activity = if (frame.start_sec - 90.).abs() < 0.12 {
            0.
        } else {
            1.
        };
    }
    for bar in &mut a.bars {
        bar.chord = Some("Am".into());
    }
    // A verse entry keeps the looped-out strip-down off this pair; the test
    // pins the long drum-layered blend for clashing keys.
    let mut b = with_stems(crate::tests::track(
        2,
        128.,
        "4A",
        SectionLabel::Verse,
        64,
        0.8,
        0.9,
    ));
    for frame in &mut b.stems.as_mut().unwrap().frames {
        frame.vocal_activity = 0.;
    }
    for bar in &mut b.bars {
        bar.chord = Some("Dm".into());
    }
    // Without stem playback the pair keeps its previous non-layered behavior.
    let plain = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(!plain.requires_stems);
    assert!(plain.stem_mix.is_none());
    // With both decks stem-ready, the tonal clash no longer forces a cut:
    // B's drums carry a long blend until the handoff releases A's foreground.
    let plan = Planner::with_options(PlannerOptions {
        stem_playback: true,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(
        plan.transition_mode,
        Some(mixless_protocol::TransitionMode::BeatBlend),
        "{:?} {:?}",
        plan.summary,
        plan.failure_reason,
    );
    let summary = plan.summary.as_ref().unwrap();
    assert_eq!(summary.strategy, mixless_protocol::StrategyId::PhraseBlend);
    let n = summary.length_bars as f32;
    assert!(n >= 8.);
    let stems = plan.stem_mix.as_ref().expect("stem-layered plan");
    assert_eq!(stems.incoming[0].sample(0.), 0.);
    assert_eq!(stems.incoming[2].sample(0.), 0.);
    assert_eq!(stems.incoming[0].sample(n), 1.);
    assert_eq!(stems.incoming[2].sample(n), 1.);
    for u in [0., n * 0.5, n] {
        assert_eq!(stems.incoming[1].sample(u), 1., "incoming drums at {u}");
        assert_eq!(stems.outgoing[1].sample(u), 1., "outgoing drums at {u}");
    }
    assert_eq!(stems.outgoing[0].sample(n), 0.);
    assert!(plan.requires_stems);
}

#[test]
fn transcribed_progression_changes_matching_with_pitch_offset_and_rejects_corruption() {
    let a = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        16,
        0.7,
        0.2,
    ));
    let mut b = a.clone();
    b.track_id = TrackId(2);
    for frame in &mut b.stems.as_mut().unwrap().frames {
        frame.note_chroma = [0.; 12];
        frame.note_chroma[1] = 1.;
    }
    let source = Polyline {
        nodes: vec![(0., 0.), (8., 16.)],
    };
    assert!(crate::progression::tension(&a, &b, &source, &source, 8., 0., 0.) > 0.7);
    assert!(crate::progression::tension(&a, &b, &source, &source, 8., 1., 0.) < 0.01);
    b.stems.as_mut().unwrap().frames[3].rms[0] = f32::NAN;
    let plan = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(
        plan.failure_reason
            .as_deref()
            .is_some_and(|e| e.contains("stem"))
    );
}

#[test]
fn unmatched_tempi_hand_the_beat_over_on_one_line_with_stems() {
    let mut a = with_stems(crate::tests::track(
        1,
        175.,
        "8A",
        SectionLabel::Drop,
        96,
        0.9,
        0.3,
    ));
    let edge = 80. * 240. / 175.;
    a.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: edge,
            label: SectionLabel::Drop,
        },
        mixless_protocol::Section {
            start_sec: edge,
            end_sec: a.duration_sec,
            label: SectionLabel::Outro,
        },
    ];
    let mut b = with_stems(crate::tests::track(
        2,
        120.,
        "8A",
        SectionLabel::Intro,
        64,
        0.9,
        0.3,
    ));
    for track in [&mut a, &mut b] {
        for frame in &mut track.stems.as_mut().unwrap().frames {
            frame.vocal_activity = 0.;
        }
        for bar in &mut track.bars {
            bar.vocal_presence = 0.;
            bar.vocal_confidence = Some(0.);
        }
    }
    let planner = Planner::with_options(PlannerOptions {
        stem_playback: true,
        ..Default::default()
    });
    let p = planner.plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.failure_reason.is_none(), "{:?}", p.failure_reason);
    let n = p.summary.as_ref().unwrap().length_bars as f32;
    if p.requires_stems && p.transition_mode == Some(mixless_protocol::TransitionMode::PhraseBridge)
    {
        let stems = p.stem_mix.as_ref().unwrap();
        // One instant: A's drums are gone within a sixteenth of a bar while
        // B is already audible at full trim on the same downbeat.
        assert!(stems.outgoing[StemKind::Drums.index()].sample(0.1) < 0.01);
        assert!(
            p.lanes.gain_b.sample(0.) > -6.,
            "{:?}",
            p.lanes.gain_b.nodes.first()
        );
        assert!(p.lanes.xfader.sample(0.) >= 0.);
        assert!(p.lanes.gain_a.sample(n) <= -90.);
        assert!((p.lanes.rate_b.sample(0.) - 1.).abs() < 1e-6, "native rate");
    } else {
        panic!(
            "expected a stem strip-out, got {:?} {:?}",
            p.transition_mode, p.summary
        );
    }
}

#[test]
fn a_vocal_stem_exchange_releases_the_incoming_mid_cut_only_with_stem_playback() {
    let a = with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        32,
        0.8,
        0.2,
    ));
    let mut b = a.clone();
    b.track_id = TrackId(2);
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let line = |nodes: &[(f32, f32)]| Polyline {
        nodes: nodes.to_vec(),
    };
    let mut plan = MixPlan {
        summary: Some(mixless_protocol::MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: mixless_protocol::StrategyId::PhraseBlend,
            score: 1.,
            used_fallback: false,
            length_bars: 16,
        }),
        clock: line(&[(0., 0.), (16., 32.)]),
        outgoing_source: line(&[(0., 0.), (16., 32.)]),
        incoming_source: line(&[(0., 0.), (16., 32.)]),
        ..Default::default()
    };
    plan.lanes.gain_a = line(&[(0., 0.), (16., -36.)]);
    plan.lanes.gain_b = line(&[(0., -36.), (16., 0.)]);
    // Whole-track vocal protection, as vocals::protect leaves it.
    plan.lanes.eq_b.mid = line(&[(0., -18.), (14., -18.), (16., 0.)]);
    let mut without = plan.clone();
    crate::stem_mix::arrange(&ctx, &mut without, false);
    assert_eq!(without.lanes.eq_b.mid.sample(13.), -18.);
    assert!(!without.requires_stems);
    crate::stem_mix::arrange(&ctx, &mut plan, true);
    let vocal = &plan.stem_mix.as_ref().unwrap().outgoing[StemKind::Vocals.index()];
    assert!(vocal.sample(13.) < 0.2);
    assert!(
        plan.lanes.eq_b.mid.sample(13.) > -3.,
        "{:?}",
        plan.lanes.eq_b.mid.sample(13.)
    );
    assert!(
        plan.lanes.eq_b.mid.sample(1.) < -17.,
        "still protected while A sings: {} vocal {}",
        plan.lanes.eq_b.mid.sample(1.),
        vocal.sample(1.)
    );
    assert!(plan.requires_stems);
}

#[test]
fn harmonic_stem_blends_swap_one_part_at_a_time_and_can_ride_the_vocal() {
    let make = |vocal_a: f32| {
        let mut a = with_stems(crate::tests::track(
            1,
            128.,
            "8A",
            SectionLabel::Outro,
            64,
            0.9,
            0.3,
        ));
        let mut b = with_stems(crate::tests::track(
            2,
            128.,
            "8A",
            SectionLabel::Intro,
            64,
            0.9,
            0.3,
        ));
        for f in &mut a.stems.as_mut().unwrap().frames {
            f.vocal_activity = vocal_a;
        }
        for f in &mut b.stems.as_mut().unwrap().frames {
            f.vocal_activity = 0.;
        }
        for bar in a.bars.iter_mut().chain(&mut b.bars) {
            bar.vocal_presence = 0.1;
        }
        (a, b)
    };
    let planner = Planner::with_options(PlannerOptions {
        stem_playback: true,
        ..Default::default()
    });
    let (a, b) = make(0.);
    let p = planner.plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let summary = p.summary.as_ref().expect("plan");
    assert_eq!(summary.strategy, StrategyId::StemSwap, "{:?}", p.stages);
    let n = summary.length_bars as f32;
    let stems = p.stem_mix.as_ref().unwrap();
    let (d, ba) = (StemKind::Drums.index(), StemKind::Bass.index());
    let handoff = p.handoff_bar.unwrap();
    // One kit at a time apart from a one-bar join, one bass at a time.
    let mut both_kits = 0;
    for i in 0..=256 {
        let u = n * i as f32 / 256.;
        if stems.outgoing[d].sample(u) > 0.5 && stems.incoming[d].sample(u) > 0.5 {
            both_kits += 1;
        }
        assert!(stems.outgoing[ba].sample(u) + stems.incoming[ba].sample(u) <= 1.01 + 0.5);
    }
    assert!(
        (both_kits as f32) < 256. / n * 1.5,
        "kits overlap {both_kits} steps"
    );
    assert!(stems.outgoing[ba].sample(handoff - 0.1) > 0.9);
    assert!(stems.incoming[ba].sample(handoff + 0.3) > 0.9);
    for stem in 0..4 {
        assert_eq!(
            stems.incoming[stem].nodes.last().unwrap().1,
            1.,
            "B ends complete"
        );
        // Stem gains hold after the plan, so every move lands inside it.
        assert!(stems.incoming[stem].sample(n) > 0.99, "stem {stem} late in");
        assert!(
            stems.outgoing[stem].sample(n) < 0.01,
            "stem {stem} late out"
        );
    }
    assert!(p.requires_stems);

    // A voice that never breathes is never cut by a swap.
    let (a, b) = make(1.);
    let p = planner.plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(!matches!(
        p.summary.as_ref().map(|s| s.strategy),
        Some(StrategyId::StemSwap | StrategyId::VocalRide)
    ));

    // A sung outro over a voiceless intro keeps the voice until its breath.
    let (mut a, b) = make(1.);
    let breath = 60. * 240. / 128.;
    for f in &mut a.stems.as_mut().unwrap().frames {
        if (f.start_sec - breath).abs() < 0.3 {
            f.vocal_activity = 0.;
        }
    }
    let p = planner.plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert_eq!(
        p.summary.as_ref().map(|s| s.strategy),
        Some(StrategyId::VocalRide)
    );
    {
        let stems = p.stem_mix.as_ref().unwrap();
        let v = StemKind::Vocals.index();
        let n = p.summary.as_ref().unwrap().length_bars as f32;
        let handoff = p.handoff_bar.unwrap();
        // Over the incoming instrumental the outgoing voice is still up,
        // the outgoing instruments are not, and the incoming voice waits.
        let u = handoff + 2.5;
        assert!(u < n - 1.);
        {
            assert!(stems.outgoing[v].sample(u) > 0.9);
            assert!(stems.outgoing[StemKind::Instruments.index()].sample(u) < 0.1);
            assert!(stems.incoming[v].sample(u) < 0.1);
            assert!(p.lanes.gain_a.sample(u) > -1.);
            assert!(p.lanes.gain_a.sample(n) <= -90., "the outgoing deck closes");
            assert!(p.lanes.xfader.sample(u) < 0.6, "fader held for the voice");
            assert!(
                stems.outgoing[v].sample(n - 0.01) < 0.01,
                "voice gone in time"
            );
            assert_eq!(
                p.send_fx.map(|fx| fx.kind),
                Some(mixless_protocol::FxKind::LowCutEcho),
                "the last word is thrown"
            );
        }
    }
}
