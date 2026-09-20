use super::*;
use mixless_protocol::{
    AutomationLanes, EqLane, FilterLane, MixPlan, MixPlanSummary, Polyline, StemMix, StrategyId,
    TransitionMode,
};

const SR: u32 = 16_000;
fn line(nodes: &[(f32, f32)]) -> Polyline {
    Polyline {
        nodes: nodes.to_vec(),
    }
}

fn fixture(stems: bool, quiet: Option<f32>, enabled: bool, outgoing: DeckId) -> Engine {
    fixture_plan(stems, quiet, enabled, outgoing, |_| {})
}

fn fixture_plan(
    stems: bool,
    quiet: Option<f32>,
    enabled: bool,
    outgoing: DeckId,
    configure: impl FnOnce(&mut MixPlan),
) -> Engine {
    let engine = test_engine(SR);
    for (relative, deck) in [outgoing, DeckId::from_index(1 - outgoing.index()).unwrap()]
        .into_iter()
        .enumerate()
    {
        let id = TrackId(relative as i64 + 1);
        let mut buffer = sine_buffer(SR, if relative == 0 { 440. } else { 550. }, 18.);
        if let Some(level) = quiet {
            let audio = Arc::get_mut(&mut buffer).unwrap();
            let start = if relative == 0 { 4. } else { 0. };
            for (frame, sample) in audio.samples.chunks_exact_mut(2).enumerate() {
                let t = frame as f32 / SR as f32;
                if t >= start && t < start + 7. {
                    for x in sample {
                        *x *= level;
                    }
                }
            }
        }
        let wave = Arc::new(crate::compute_preview_waveform(&buffer, 256));
        engine
            .load_buffer_if(
                deck,
                id,
                buffer.clone(),
                wave,
                String::new(),
                String::new(),
                || true,
            )
            .unwrap();
        if stems {
            let vocals = buffer.samples.iter().map(|v| *v * 0.75).collect();
            let drums = buffer.samples.iter().map(|v| *v * 0.25).collect();
            let separated = Arc::new(crate::StemBuffer::new(SR, vocals, drums).unwrap());
            assert!(engine.attach_stems(deck, id, &buffer, separated).unwrap());
        }
    }
    let mut plan = MixPlan {
        summary: Some(MixPlanSummary {
            pair: (TrackId(1), TrackId(2)),
            strategy: StrategyId::PhraseBlend,
            score: 0.8,
            used_fallback: false,
            length_bars: 16,
        }),
        t_in_a: 4.,
        t_out_a: 12.,
        t_in_b: 0.,
        t_end_b: 8.,
        clock: line(&[(0., 0.), (16., 8.)]),
        incoming_source: line(&[(0., 0.), (16., 8.)]),
        outgoing_source: line(&[(0., 4.), (16., 12.)]),
        transition_mode: Some(TransitionMode::BeatBlend),
        master_bpm: Polyline::constant(120.),
        lanes: AutomationLanes {
            xfader: line(&[(0., -1.), (16., 1.)]),
            gain_a: line(&[(0., 0.), (15.9, 0.), (16., -96.)]),
            gain_b: Polyline::constant(0.),
            eq_a: EqLane {
                low: Polyline::constant(0.),
                mid: Polyline::constant(0.),
                high: Polyline::constant(0.),
            },
            eq_b: EqLane {
                low: Polyline::constant(0.),
                mid: Polyline::constant(0.),
                high: Polyline::constant(0.),
            },
            filter_a: FilterLane {
                lp_hz: Polyline::constant(20000.),
                hp_hz: Polyline::constant(20.),
            },
            filter_b: FilterLane {
                lp_hz: Polyline::constant(20000.),
                hp_hz: Polyline::constant(20.),
            },
            fx_send_a: Polyline::constant(0.),
            fx_send_b: Polyline::constant(0.),
            rate_a: Polyline::constant(1.),
            rate_b: Polyline::constant(1.),
            pitch_a: Polyline::constant(0.),
            pitch_b: Polyline::constant(0.),
            ..Default::default()
        },
        ..Default::default()
    };
    if stems {
        plan.stem_mix = Some(StemMix {
            outgoing: std::array::from_fn(|_| Polyline::constant(1.)),
            incoming: std::array::from_fn(|i| {
                if i == 1 {
                    Polyline::constant(1.)
                } else {
                    line(&[(0., 0.), (12., 0.), (16., 1.)])
                }
            }),
        });
    }
    configure(&mut plan);
    engine.dispatch(Command::SetMaster { value: 1. }).unwrap();
    engine
        .dispatch(Command::SetCrossfader {
            value: if outgoing == DeckId::A { -1. } else { 1. },
        })
        .unwrap();
    engine
        .cue_loaded_track(outgoing, TrackId(1), 4 * SR as u64)
        .unwrap();
    engine
        .dispatch(Command::PlayPause { deck: outgoing })
        .unwrap();
    engine.load_plan_on(plan, outgoing).unwrap();
    if !enabled {
        // Manual GAIN takes over compensation without cancelling the mix.
        engine
            .dispatch(Command::SetDeckLimiterGain {
                deck: outgoing,
                db: 0.,
            })
            .unwrap();
    }
    engine
}

#[test]
fn automix_gain_fills_stem_and_quiet_overlap_dips_without_accumulating() {
    for (stems, quiet) in [(true, None), (false, Some(0.4))] {
        for outgoing in [DeckId::A, DeckId::B] {
            let compensated = fixture(stems, quiet, true, outgoing);
            let baseline = fixture(stems, quiet, false, outgoing);
            for e in [&compensated, &baseline] {
                e.render_offline(4 * SR as usize);
            }
            let before = baseline.render_offline(SR as usize);
            let after = compensated.render_offline(SR as usize);
            let improvement = 10. * (energy(&after) / energy(&before)).log10();
            assert!(improvement > 1.5, "stems={stems} improvement={improvement}");
            let snap = compensated.snapshot();
            assert!(snap.automix_on);
            assert!(snap
                .decks
                .iter()
                .all(|d| (0. ..=6.).contains(&d.automix_gain_db) && d.limiter_gain_db == 0.));
            assert!(after.iter().all(|v| v.is_finite() && v.abs() <= 0.980001));
            compensated.render_offline(5 * SR as usize);
            let finished = compensated.snapshot();
            assert!(!finished.automix_on && finished.automix_progress == 1.);
            assert!(finished
                .decks
                .iter()
                .all(|d| d.automix_gain_db == 0. && d.limiter_gain_db == 0.));
            let gain = compensated.rt.lock().unwrap().decks[1 - outgoing.index()]
                .automix_gain
                .next();
            assert!((gain - 1.).abs() < 1e-6);
        }
    }
}

#[test]
fn automix_gain_ends_at_handoff_before_a_solo_tempo_tail() {
    let engine = fixture_plan(false, None, true, DeckId::A, |plan| {
        plan.clock.nodes.push((24., 12.));
        plan.lanes.eq_b.mid = Polyline::constant(-6.);
    });
    engine.render_offline(6 * SR as usize);
    assert!(engine.snapshot().decks[1].automix_gain_db > 1.);
    engine.render_offline(3 * SR as usize);
    let snap = engine.snapshot();
    assert!(snap.automix_on && snap.decks[1].playing);
    assert!(snap.decks.iter().all(|d| d.automix_gain_db == 0.));
}

#[test]
fn automix_gain_leaves_balanced_audio_silence_and_noise_alone() {
    for quiet in [None, Some(0.), Some(0.0001)] {
        let engine = fixture(false, quiet, true, DeckId::A);
        engine.render_offline(4 * SR as usize);
        assert!(
            engine
                .snapshot()
                .decks
                .iter()
                .all(|d| d.automix_gain_db == 0.),
            "{quiet:?}"
        );
    }
}

#[test]
fn automix_gain_pause_resume_skip_and_manual_takeover_are_bounded() {
    let engine = fixture(true, None, true, DeckId::A);
    engine.render_offline(4 * SR as usize);
    assert!(engine.snapshot().decks[0].automix_gain_db > 1.);
    engine.dispatch(Command::PauseAutomix).unwrap();
    let pause = engine.render_offline(SR as usize);
    assert!(energy(&pause[pause.len() / 2..]) < 1e-9);
    assert!(engine
        .snapshot()
        .decks
        .iter()
        .all(|d| d.automix_gain_db == 0.));
    engine.dispatch(Command::ResumeAutomix).unwrap();
    engine.render_offline(SR as usize / 2);
    assert!(engine.snapshot().decks[0].automix_gain_db > 1.);
    engine
        .dispatch(Command::SetDeckLimiterGain {
            deck: DeckId::A,
            db: 2.,
        })
        .unwrap();
    engine.render_offline(SR as usize);
    let manual = engine.snapshot();
    assert!(manual.automix_on);
    assert_eq!(manual.decks[0].limiter_gain_db, 2.);
    assert!(manual.decks.iter().all(|d| d.automix_gain_db == 0.));
    engine.dispatch(Command::SkipAutomix).unwrap();
    engine.render_offline(SR as usize);
    assert!(!engine.snapshot().automix_on);
    assert!(engine
        .snapshot()
        .decks
        .iter()
        .all(|d| d.automix_gain_db == 0.));
}

#[test]
fn automix_gain_knob_target_tracks_compensation_and_manual_takeover() {
    for outgoing in [DeckId::A, DeckId::B] {
        for controlled in [DeckId::A, DeckId::B] {
            let engine = fixture(true, None, true, outgoing);
            assert_eq!(
                engine
                    .snapshot()
                    .deck(controlled)
                    .effective_limiter_gain_db(),
                0.
            );
            engine.render_offline(4 * SR as usize);
            let automatic = engine
                .snapshot()
                .deck(controlled)
                .effective_limiter_gain_db();
            assert!(automatic > 1. && automatic <= 6.);
            // The UI starts dragging at the displayed effective value. The
            // manual command replaces AUTO, so this must preserve the target.
            engine
                .dispatch(Command::SetDeckLimiterGain {
                    deck: controlled,
                    db: automatic,
                })
                .unwrap();
            engine.render_offline(256);
            let snapshot = engine.snapshot();
            assert!(snapshot.automix_on);
            assert!(snapshot.decks.iter().all(|d| d.automix_gain_db == 0.));
            assert!(
                (snapshot.deck(controlled).effective_limiter_gain_db() - automatic).abs() < 0.011
            );
        }
        let engine = fixture(true, None, true, outgoing);
        engine.render_offline(4 * SR as usize);
        assert!(engine.snapshot().deck(outgoing).effective_limiter_gain_db() > 1.);
        engine.dispatch(Command::StopAutomix).unwrap();
        engine.render_offline(SR as usize);
        assert!(engine
            .snapshot()
            .decks
            .iter()
            .all(|d| d.effective_limiter_gain_db() == 0.));
    }
}

#[test]
fn automix_gain_manual_takeover_cannot_overshoot_the_deck_gain_range() {
    let engine = fixture(false, Some(0.4), true, DeckId::A);
    engine.render_offline(4 * SR as usize);
    assert!(engine.snapshot().decks[0].automix_gain_db > 2.);
    engine
        .dispatch(Command::SetDeckLimiterGain {
            deck: DeckId::A,
            db: 12.,
        })
        .unwrap();
    engine.render_offline(256);
    // Source peak is 0.2 here; +12 dB is <0.797. Without the per-sample
    // combined-gain cap, the slow AUTO release would drive it into limiting.
    assert!(engine.snapshot().decks[0]
        .level
        .iter()
        .all(|peak| *peak < 0.8));
}

#[test]
fn automix_gain_callback_does_not_allocate_and_stop_releases_it() {
    let engine = fixture(true, None, true, DeckId::A);
    engine.render_offline(4 * SR as usize);
    let mut rt = engine.rt.lock().unwrap();
    let mut output = [0.; 512];
    assert_eq!(
        super::allocation::measure(|| engine.shared.process_block(&mut rt, &mut output, 2)),
        [0; 3]
    );
    drop(rt);
    engine.dispatch(Command::StopAutomix).unwrap();
    engine.render_offline(SR as usize);
    assert!(engine
        .snapshot()
        .decks
        .iter()
        .all(|d| d.automix_gain_db == 0. && d.limiter_gain_db == 0.));
}
