use super::*;
use mixless_protocol::{
    MixPlanSummary, Polyline, Section, SectionLabel as S, StemMix, StrategyId, TrackAnalysis,
};

fn fixture(bpm: f32, gain: f32, offset: f32) -> (TrackAnalysis, MixPlan) {
    let bar = 240. / bpm;
    let mut b = crate::tests::track(2, bpm, "8A", S::Intro, 64, 0.8, 0.4);
    b.stems = crate::vocals::chop_tests::track(false).stems;
    let template = b.stems.as_ref().unwrap().frames[0].clone();
    let mut frames = Vec::new();
    for i in 0..64 * 32 {
        let mut f = template.clone();
        f.start_sec = offset + i as f32 * bar / 32.;
        f.end_sec = offset + (i + 1) as f32 * bar / 32.;
        f.band_db = [[-100.; 3], [-15.; 3], [-15.; 3]];
        for db in f.band_db.iter_mut().flatten() {
            *db += 20. * gain.log10();
        }
        f.vocal_activity = 0.;
        frames.push(f);
    }
    b.stems.as_mut().unwrap().frames = frames;
    b.stems.as_mut().unwrap().notes.clear();
    b.duration_sec += offset;
    b.stems.as_mut().unwrap().duration_sec = b.duration_sec;
    for beat in b.tempo.beats.iter_mut().chain(&mut b.tempo.downbeats) {
        *beat += offset;
    }
    for f in &mut b.bars {
        f.start_sec += offset;
        f.end_sec += offset;
    }
    b.sections = [
        (0., 8., S::Intro),
        (8., 24., S::Drop),
        (24., 32., S::Break),
        (32., 64., S::Drop),
    ]
    .map(|(a, z, label)| Section {
        start_sec: offset + a * bar,
        end_sec: offset + z * bar,
        label,
    })
    .to_vec();
    let mut p = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (b.track_id, b.track_id),
            strategy: StrategyId::StemSwap,
            score: 1.,
            used_fallback: false,
            length_bars: 16,
        }),
        t_in_b: offset,
        t_end_b: offset + 16. * bar,
        t_in_a: offset,
        t_out_a: offset + 16. * bar,
        incoming_source: Polyline {
            nodes: vec![(0., offset), (16., offset + 16. * bar)],
        },
        outgoing_source: Polyline {
            nodes: vec![(0., offset), (16., offset + 16. * bar)],
        },
        clock: Polyline {
            nodes: vec![(0., 0.), (16., 16. * bar)],
        },
        stem_mix: Some(StemMix {
            outgoing: std::array::from_fn(|_| Polyline::constant(0.)),
            incoming: std::array::from_fn(|_| Polyline::constant(1.)),
        }),
        ..Default::default()
    };
    p.lanes.xfader = Polyline::constant(1.);
    for filter in [&mut p.lanes.filter_a, &mut p.lanes.filter_b] {
        filter.hp_hz = Polyline::constant(20.);
        filter.lp_hz = Polyline::constant(20000.);
    }
    (b, p)
}

#[test]
fn silent_voice_and_delayed_bass_do_not_hide_an_established_melody() {
    for bpm in [90., 120., 175.] {
        for gain in [0.1, 1., 3.] {
            for offset in [0., 37.] {
                let (b, mut p) = fixture(bpm, gain, offset);
                p.stem_mix.as_mut().unwrap().incoming[0] = Polyline::constant(0.);
                p.stem_mix.as_mut().unwrap().incoming[3] = Polyline::constant(0.);
                assert_eq!(established_entry(&p, &b), offset);
                p.lanes.gain_b = Polyline::constant(-18.);
                assert_eq!(established_entry(&p, &b), offset);
                let mut unknown = b.clone();
                unknown.stems = None;
                assert!(established_entry(&p, &unknown) > offset + 15. * 240. / bpm);
            }
        }
    }
}

#[test]
fn loud_kit_cannot_credit_a_muted_melody_or_singer() {
    let (mut b, mut p) = fixture(120., 1., 0.);
    let late = Polyline {
        nodes: vec![(0., 0.), (12., 0.), (14., 1.), (16., 1.)],
    };
    p.stem_mix.as_mut().unwrap().incoming[2] = late.clone();
    assert!(established_entry(&p, &b) > 24.);
    p.stem_mix.as_mut().unwrap().incoming[2] = Polyline::constant(1.);
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.band_db[0] = [-20.; 3];
        f.vocal_activity = 0.9;
    }
    p.stem_mix.as_mut().unwrap().incoming[0] = late;
    assert!(established_entry(&p, &b) > 24.);
    p.stem_mix.as_mut().unwrap().incoming[0] = Polyline::constant(1.);
    p.lanes.eq_b.mid = Polyline {
        nodes: vec![(0., -24.), (12., -24.), (14., 0.), (16., 0.)],
    };
    p.lanes.eq_b.high = p.lanes.eq_b.mid.clone();
    assert!(established_entry(&p, &b) > 24.);
}

#[test]
fn peak_opening_credits_its_own_bar_but_not_a_drum_only_preview() {
    for bpm in [90., 120., 175.] {
        let (b, mut p) = fixture(bpm, 1., 0.);
        let bar = 240. / bpm;
        p.stem_mix.as_mut().unwrap().incoming[2] = Polyline {
            nodes: vec![(0., 0.), (7.9, 0.), (8.2, 1.), (16., 1.)],
        };
        assert!((established_entry(&p, &b) - 8. * bar).abs() < 0.001);
        assert_eq!(arrival_debt(&p, &b), 0.);
        p.stem_mix.as_mut().unwrap().incoming[2] = Polyline {
            nodes: vec![(0., 0.), (8.9, 0.), (10., 1.), (16., 1.)],
        };
        assert!(established_entry(&p, &b) > 9. * bar);
        assert!(arrival_debt(&p, &b) > 0.);
    }
}

#[test]
fn foreground_gaps_are_not_hidden_by_a_long_reservation_or_loud_drums() {
    let (a, mut p) = fixture(120., 1., 0.);
    let mut b = a.clone();
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.band_db[2] = [-100.; 3];
    }
    p.lanes.xfader = Polyline::constant(-1.);
    let mix = p.stem_mix.as_mut().unwrap();
    mix.outgoing = std::array::from_fn(|_| Polyline::constant(1.));
    mix.incoming = std::array::from_fn(|_| Polyline::constant(0.));
    assert_eq!(continuity(&p, &a, &b).unwrap().foreground_gap_bars, 0.);
    p.stem_mix.as_mut().unwrap().outgoing[2] = Polyline {
        nodes: vec![
            (0., 1.),
            (4., 1.),
            (4.01, 0.),
            (6., 0.),
            (6.01, 1.),
            (16., 1.),
        ],
    };
    let loss = continuity(&p, &a, &b).unwrap();
    assert!((loss.foreground_gap_bars - 2.).abs() < 0.05);
    assert!((loss.longest_foreground_gap_bars - 2.).abs() < 0.05);
    assert!(needs_repair(&p, &a, &b));
    assert!(loss.gap_cost() > 0.1);
    let mut natural = a.clone();
    for f in &mut natural.stems.as_mut().unwrap().frames {
        if f.start_sec >= 8. && f.end_sec <= 12. {
            f.band_db[2] = [-100.; 3];
        }
    }
    assert!(continuity(&p, &natural, &b).unwrap().foreground_gap_bars < 0.05);
    assert!(!needs_repair(&p, &natural, &b));
    b.stems = None;
    assert!(continuity(&p, &a, &b).is_none());
}

#[test]
fn scattered_short_dips_do_not_require_the_same_order_repair_as_a_sustained_hole() {
    let (a, mut p) = fixture(120., 1., 0.);
    let b = a.clone();
    p.lanes.xfader = Polyline::constant(-1.);
    let mix = p.stem_mix.as_mut().unwrap();
    mix.outgoing = std::array::from_fn(|_| Polyline::constant(1.));
    mix.incoming = std::array::from_fn(|_| Polyline::constant(0.));
    let mut nodes = vec![(0., 1.)];
    for i in 0..8 {
        let start = i as f32 * 2. + 0.5;
        nodes.extend([
            (start, 1.),
            (start + 0.01, 0.),
            (start + 0.25, 0.),
            (start + 0.26, 1.),
        ]);
    }
    nodes.push((16., 1.));
    mix.outgoing[2] = Polyline { nodes };
    let observed = continuity(&p, &a, &b).unwrap();
    assert!(observed.foreground_gap_bars > 1.);
    assert!(observed.longest_foreground_gap_bars < 0.4);
    assert!(!needs_repair(&p, &a, &b));
}

#[test]
fn brief_foreground_attacks_do_not_hide_a_bar_level_retreat() {
    for bpm in [90., 120., 175.] {
        for gain in [0.1, 1., 3.] {
            for offset in [0., 37.] {
                let (a, mut p) = fixture(bpm, gain, offset);
                let b = a.clone();
                p.lanes.xfader = Polyline::constant(-1.);
                let mix = p.stem_mix.as_mut().unwrap();
                mix.outgoing = std::array::from_fn(|_| Polyline::constant(1.));
                mix.incoming = std::array::from_fn(|_| Polyline::constant(0.));
                let mut nodes = Vec::new();
                for i in 0..64 {
                    let at = i as f32 * 0.25;
                    nodes.extend([
                        (at, 1.),
                        (at + 0.03, 1.),
                        (at + 0.031, 0.),
                        (at + 0.249, 0.),
                    ]);
                }
                nodes.push((16., 1.));
                mix.outgoing[2] = Polyline { nodes };
                let c = continuity(&p, &a, &b).unwrap();
                assert!(c.foreground_gap_bars > 8.);
                assert!(c.longest_foreground_gap_bars < 0.5);
                assert!(minimum_bar_foreground_retention(&p, &a, &b).unwrap() < 0.25);
                assert!(needs_repair(&p, &a, &b), "{bpm} {gain} {offset}");
                // A quieter but sustained foreground is a different case.
                p.stem_mix.as_mut().unwrap().outgoing[2] = Polyline::constant(0.7);
                assert!(!needs_repair(&p, &a, &b));
            }
        }
    }
}

#[test]
fn losing_one_source_impact_does_not_invent_a_sustained_missing_part() {
    let (mut a, mut p) = fixture(120., 1., 0.);
    for (i, frame) in a.stems.as_mut().unwrap().frames.iter_mut().enumerate() {
        if i != 128 {
            frame.band_db[2] = [-100.; 3];
        }
    }
    let b = a.clone();
    p.lanes.xfader = Polyline::constant(-1.);
    let mix = p.stem_mix.as_mut().unwrap();
    mix.outgoing = std::array::from_fn(|_| Polyline::constant(1.));
    mix.outgoing[2] = Polyline::constant(0.);
    mix.incoming = std::array::from_fn(|_| Polyline::constant(0.));
    assert!(continuity(&p, &a, &b).unwrap().foreground_gap_bars < 0.05);
    assert!(!needs_repair(&p, &a, &b));
}

#[test]
fn isolated_drum_hits_do_not_turn_into_sustained_overlap() {
    let (a, mut p) = fixture(120., 1., 0.);
    let mut b = a.clone();
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.band_db = [[-100.; 3]; 3];
        if (f.start_sec / 2.).fract() < 0.031 {
            f.band_db[1] = [-15.; 3];
        }
    }
    p.lanes.xfader = Polyline::constant(0.);
    p.stem_mix.as_mut().unwrap().outgoing = std::array::from_fn(|_| Polyline::constant(1.));
    let c = continuity(&p, &a, &b).unwrap();
    assert!(c.overlap_bars > 0.2 && c.overlap_bars < 1.);
    assert_eq!(c.foreground_gap_bars, 0.);
}

#[test]
fn terminal_quality_uses_the_heard_exit_and_includes_ordinary_dry_cuts() {
    let (a, mut p) = fixture(120., 1., 0.);
    let b = a.clone();
    p.t_in_a = 96.;
    p.t_out_a = 128.;
    p.outgoing_source.nodes = vec![(0., 96.), (16., 128.)];
    p.lanes.xfader.nodes = vec![(0., -1.), (4., 1.), (16., 1.)];
    p.stem_mix.as_mut().unwrap().outgoing = std::array::from_fn(|_| Polyline::constant(1.));
    assert!(
        !terminal_handoff(&p, &a, &b),
        "a residual reservation is not a tail handoff"
    );
    p.summary.as_mut().unwrap().length_bars = 0;
    p.summary.as_mut().unwrap().strategy = StrategyId::DryCut;
    p.t_in_a = 124.;
    p.t_out_a = 124.;
    assert!(!p.summary.as_ref().unwrap().used_fallback);
    assert!(terminal_handoff(&p, &a, &b));
    let mut overlap = p.clone();
    overlap.summary.as_mut().unwrap().length_bars = 8;
    overlap.summary.as_mut().unwrap().strategy = StrategyId::StemSwap;
    overlap.summary.as_mut().unwrap().score = 0.55;
    overlap.t_in_a = 80.;
    overlap.t_out_a = 96.;
    overlap.outgoing_source.nodes = vec![(0., 80.), (8., 96.)];
    overlap.incoming_source.nodes = vec![(0., 0.), (8., 16.)];
    overlap.lanes.xfader.nodes = vec![(0., -1.), (8., 1.)];
    p.summary.as_mut().unwrap().score = 0.6;
    assert!(
        merit(&overlap, &a, &b) > merit(&p, &a, &b),
        "current quality must survive raw-score pruning"
    );
}
