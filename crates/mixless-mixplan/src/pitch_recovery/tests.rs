use super::*;
use mixless_protocol::{Section, SectionLabel as S, StemAnalysis, StemFrame, StemKind, StemNote};

fn source() -> TrackAnalysis {
    let mut t = crate::tests::track(2, 120., "8A", S::Drop, 64, 0.8, 1.);
    t.sections = vec![
        Section {
            start_sec: 0.,
            end_sec: 16.,
            label: S::Drop,
        },
        Section {
            start_sec: 16.,
            end_sec: 48.,
            label: S::Breakdown,
        },
        Section {
            start_sec: 48.,
            end_sec: 128.,
            label: S::Drop,
        },
    ];
    for bar in &mut t.bars {
        if (16. ..48.).contains(&bar.start_sec) {
            bar.rms = 0.2;
            bar.section = S::Breakdown;
        }
    }
    t.stems = Some(StemAnalysis {
        version: 2,
        separator_sha256: "fixture".into(),
        notes_sha256: "fixture".into(),
        duration_sec: t.duration_sec,
        residual_rms: 0.,
        notes: vec![],
        frames: (0..2560)
            .map(|i| StemFrame {
                start_sec: i as f32 * 0.05,
                end_sec: (i + 1) as f32 * 0.05,
                rms: [0.0001, 0.03, 0.05],
                band_db: [[-40.; 3]; 3],
                onset: [0.; 3],
                drum_low_onset: 0.,
                vocal_activity: 0.,
                note_chroma: [0.; 12],
            })
            .collect(),
    });
    t
}

fn base() -> MixPlan {
    let mut p = MixPlan::default();
    p.transition_mode = Some(mixless_protocol::TransitionMode::BeatBlend);
    p.clock = Polyline {
        nodes: vec![(0., 0.), (4., 8.)],
    };
    p.t_end_b = 8.;
    p.incoming_offset_end = PerformanceOffset {
        rate: 1.,
        pitch_semitones: -3.,
    };
    p.lanes.xfader = Polyline {
        nodes: vec![(0., -1.), (4., 1.)],
    };
    p.lanes.rate_b = Polyline::constant(1.);
    p.lanes.pitch_b = Polyline::constant(-3.);
    p.lanes.filter_b.lp_hz = Polyline::constant(20000.);
    p.lanes.filter_b.hp_hz = Polyline::constant(20.);
    p
}

fn restore(t: &TrackAnalysis, p: &mut MixPlan, manual: f32) {
    let planner = Planner::with_options(crate::PlannerOptions {
        harmonic_key_shift: true,
        stem_playback: true,
        ..Default::default()
    });
    arrange(
        &planner,
        &PlanContext {
            outgoing: t,
            incoming: t,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: PerformanceOffset {
                pitch_semitones: manual,
                rate: 1.,
            },
        },
        None,
        &[],
        &[],
        p,
    );
}

#[test]
fn solo_recovery_preserves_overlap_and_finishes_with_neutral_controls() {
    let t = source();
    let mut p = base();
    let original = p.clone();
    restore(&t, &mut p, 0.);
    assert_eq!(p.incoming_offset_end.pitch_semitones, 0.);
    assert!(p.t_end_b > original.t_end_b);
    for i in 0..=256 {
        let u = i as f32 / 64.;
        assert_eq!(p.lanes.pitch_b.sample(u), -3.);
        assert_eq!(p.lanes.xfader.sample(u), original.lanes.xfader.sample(u));
    }
    let end = p.clock.nodes.last().unwrap().0;
    assert_eq!(p.lanes.pitch_b.sample(end), 0.);
    assert_eq!(p.lanes.eq_b.low.sample(end), 0.);
    assert!((p.lanes.filter_b.lp_hz.sample(end) - 20000.).abs() < 0.05);
    assert!(
        p.stem_mix
            .as_ref()
            .unwrap()
            .incoming
            .iter()
            .all(|l| l.sample(end) == 1.)
    );
    for pair in p.lanes.pitch_b.nodes.windows(2) {
        let seconds = p.clock.sample(pair[1].0) - p.clock.sample(pair[0].0);
        assert!((pair[1].1 - pair[0].1).abs() <= seconds + 1e-5);
    }
}

#[test]
fn preserves_manual_tuning_and_has_no_mandatory_restoration() {
    let mut t = source();
    let mut p = base();
    restore(&t, &mut p, -1.);
    assert_eq!(p.incoming_offset_end.pitch_semitones, -1.);
    for frame in &mut t.stems.as_mut().unwrap().frames {
        frame.rms[0] = 0.1;
        frame.vocal_activity = 1.;
    }
    let mut p = base();
    restore(&t, &mut p, 0.);
    assert_eq!(p.t_end_b, 8.);
    t.stems = None;
    let mut p = base();
    restore(&t, &mut p, 0.);
    assert_eq!(p.incoming_offset_end.pitch_semitones, -3.);
}

#[test]
fn rejects_sustained_notes_peak_energy_and_insufficient_time() {
    let mut t = source();
    t.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Instruments,
        start_sec: 16.,
        end_sec: 48.,
        midi: 60,
        confidence: 0.9,
    });
    assert!(window::find(&t, 10., 60., 1., 3.).is_none());
    t.stems.as_mut().unwrap().notes.clear();
    for bar in &mut t.bars {
        bar.rms = 1.;
    }
    assert!(window::find(&t, 10., 60., 1., 3.).is_none());
    assert!(window::find(&source(), 16., 21., 1., 3.).is_none());
    let mut missing = source();
    missing
        .bars
        .retain(|b| b.start_sec < 16. || b.start_sec >= 48.);
    assert!(window::find(&missing, 10., 60., 1., 1.).is_none());
    let mut uncertain = source();
    uncertain.tempo.segments[0].confidence = 0.2;
    assert!(window::find(&uncertain, 10., 60., 1., 1.).is_none());
}

#[test]
fn window_is_invariant_to_common_gain_and_source_time_translation() {
    let t = source();
    let w = window::find(&t, 10., 60., 1., 3.).unwrap();
    let mut fallback = t.clone();
    fallback.partial = true;
    assert!(window::find(&fallback, 10., 60., 1., 3.).is_some());
    for gain in [0.1, 3.] {
        let mut scaled = t.clone();
        for bar in &mut scaled.bars {
            bar.rms *= gain;
        }
        for f in &mut scaled.stems.as_mut().unwrap().frames {
            f.rms = f.rms.map(|r| r * gain);
        }
        let got = window::find(&scaled, 10., 60., 1., 3.).unwrap();
        assert_eq!((w.start, w.end), (got.start, got.end));
    }
    // Pad a source with four seconds rather than changing its internal timing.
    let mut shifted = t.clone();
    shifted.duration_sec += 4.;
    for s in &mut shifted.sections {
        s.start_sec += 4.;
        s.end_sec += 4.;
    }
    for b in &mut shifted.bars {
        b.start_sec += 4.;
        b.end_sec += 4.;
    }
    for time in shifted
        .tempo
        .beats
        .iter_mut()
        .chain(&mut shifted.tempo.downbeats)
    {
        *time += 4.;
    }
    let stems = shifted.stems.as_mut().unwrap();
    stems.duration_sec += 4.;
    let mut padding = stems.frames[..80].to_vec();
    for f in &mut stems.frames {
        f.start_sec += 4.;
        f.end_sec += 4.;
    }
    padding.append(&mut stems.frames);
    stems.frames = padding;
    let got = window::find(&shifted, 14., 64., 1., 3.).unwrap();
    assert_eq!((w.start + 4., w.end + 4.), (got.start, got.end));
}

#[test]
fn future_key_and_explicit_departure_change_the_restoration_decision() {
    let t = source();
    let planner = Planner::with_options(crate::PlannerOptions {
        harmonic_key_shift: true,
        ..Default::default()
    });
    let ctx = PlanContext {
        outgoing: &t,
        incoming: &t,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let mut results = Vec::new();
    for number in 1..=12 {
        let key = format!("{number}A");
        let c = crate::tests::track(3, 120., &key, S::Intro, 64, 0.8, 1.);
        let mut p = base();
        arrange(&planner, &ctx, Some(&c), &[], &[], &mut p);
        results.push(p.incoming_offset_end.pitch_semitones);
        let next = departure(
            &planner,
            &t,
            &c,
            &p,
            p.incoming_offset_end.pitch_semitones,
            &[],
            &[],
        )
        .unwrap();
        assert!(next.t_in_a >= p.t_end_b + p.incoming_offset_end.rate);
    }
    assert_eq!(results[7], 0.);
    assert!(results.iter().any(|p| *p < 0. && *p > -3.), "{results:?}");
    let out = Cue {
        index: 0,
        frame: 24 * 48000,
        kind: mixless_protocol::CueKind::Out,
        user_set: true,
    };
    let c = crate::tests::track(3, 120., "8A", S::Intro, 64, 0.8, 1.);
    let mut p = base();
    arrange(&planner, &ctx, Some(&c), &[out.clone()], &[], &mut p);
    assert_eq!(p.t_end_b, 8.);
    arrange(&planner, &ctx, None, &[out], &[], &mut p);
    assert_eq!(p.t_end_b, 8.);
}

#[test]
fn sustained_instrument_bed_allows_only_a_slow_small_move_and_vocals_veto_it() {
    let mut t = source();
    let note = StemNote {
        stem: StemKind::Instruments,
        start_sec: 16.,
        end_sec: 48.,
        midi: 60,
        confidence: 0.9,
    };
    t.stems.as_mut().unwrap().notes.push(note);
    let w = window::find(&t, 10., 60., 1., 1.).unwrap();
    assert!((w.end - w.start) * 0.6 >= 7.5);
    assert!(window::find(&t, 10., 60., 1., 2.).is_none());
    t.stems.as_mut().unwrap().notes[0].stem = StemKind::Vocals;
    assert!(window::find(&t, 10., 60., 1., 1.).is_none());
}

#[test]
#[ignore = "requires a local analysis JSON; prints evidence windows, not listening judgments"]
fn audit_local_pitch_windows() {
    let file = std::env::var("MIXLESS_PITCH_AUDIT_INPUT").unwrap();
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    for t in tracks {
        for shift in [1., 3.] {
            if let Some(w) = window::find(&t, 0., t.duration_sec, 1., shift) {
                println!(
                    "{} shift={shift} window={:.3}..{:.3}",
                    t.track_id.0, w.start, w.end
                );
            }
        }
    }
}
