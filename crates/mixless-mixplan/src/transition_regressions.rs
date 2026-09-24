use super::{close, measured_phrases, track};
use crate::{Planner, PlannerOptions};
use mixless_protocol::{Section, SectionLabel as S, StrategyId};

#[test]
fn short_peak_suspension_is_not_an_exit_even_at_the_previous_peak_end() {
    for bars in [1, 2, 4, 5, 6] {
        let (mut a, b) = structured(bars);
        let start = a.sections[2].end_sec;
        let end = a.sections[3].end_sec;
        // One breath, without the fixture helper's independent middle phrase.
        a.phrase_boundaries
            .retain(|p| p.time_sec <= start + 0.01 || p.time_sec >= end - 0.01);
        for bar in &mut a.bars {
            if bar.start_sec >= start - 0.01 && bar.end_sec <= end + 0.01 {
                bar.rms *= 0.25;
                bar.low_db -= 12.;
                bar.onset_density *= 0.25;
            }
        }
        assert_eq!(
            mixless_protocol::drop_suspension(&a, start),
            Some((start, end))
        );
        let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        assert!(p.summary.is_some(), "{bars} bars: {:?}", p.failure_reason);
        assert!(
            p.t_out_a > end + 0.05,
            "{bars}-bar breath cut at {} before resolution {end}",
            p.t_out_a
        );
        let fallback = crate::short_handoff(
            &crate::PlanContext {
                outgoing: &a,
                incoming: &b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            },
            start,
        );
        assert!(fallback.summary.is_some(), "{:?}", fallback.failure_reason);
        assert!(
            fallback.t_out_a > end + 0.05,
            "fallback cut at {}",
            fallback.t_out_a
        );
    }
    let (a, _) = structured(8);
    assert!(mixless_protocol::drop_suspension(&a, a.sections[2].end_sec).is_none());
}

#[test]
fn first_drop_exit_is_preferred_but_a_late_pass_still_has_a_transition() {
    let mut a = track(1, 120., "8A", S::Outro, 96, 0.8, 0.2);
    let b = track(2, 120., "9A", S::Intro, 96, 0.8, 0.2);
    a.sections = [
        (0., 32., S::Intro),
        (32., 64., S::Drop),
        (64., 96., S::Break),
        (96., 128., S::Drop),
        (128., 192., S::Outro),
    ]
    .into_iter()
    .map(|(start_sec, end_sec, label)| Section {
        start_sec,
        end_sec,
        label,
    })
    .collect();
    for bar in &mut a.bars {
        bar.section = a
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec && bar.start_sec < s.end_sec)
            .unwrap()
            .label;
    }
    measured_phrases(&mut a, &[0, 8, 16, 24, 32, 40, 48, 56, 64, 72, 80, 88, 96]);
    let plan = |earliest, entry| {
        Planner::with_options(PlannerOptions {
            earliest_outgoing_sec: earliest,
            outgoing_entry_sec: entry,
            ..Default::default()
        })
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default())
    };
    let p = plan(0., 0.);
    assert!(p.summary.is_some(), "{:?}", p.failure_reason);
    assert!((64. ..=96.).contains(&p.t_out_a), "exit {}", p.t_out_a);
    let late = plan(110., 0.);
    assert!(late.summary.is_some() && late.t_in_a >= 110.);
    assert_eq!(crate::drops::penalty(&a, 128., 96.), 0.);
    assert!(crate::drops::penalty(&a, 128., 0.) > 0.3);
}

#[test]
fn incompatible_drums_use_a_true_cut_instead_of_a_one_bar_filter_or_echo_blend() {
    for label in [S::Drop, S::Outro] {
        let a = track(1, 128., "8A", label, 64, 0.8, 0.2);
        let b = track(2, 105., "3B", S::Intro, 64, 0.8, 0.2);
        let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        let summary = p.summary.as_ref().unwrap();
        assert_eq!(summary.strategy, StrategyId::DryCut);
        let n = summary.length_bars as f32;
        close(p.incoming_start_bar, n);
        close(n, 0.);
        close(p.t_in_a, p.t_out_a);
        close(p.lanes.xfader.sample(n), 1.);
        close(p.lanes.fx_send_a.sample(n), 0.);
        close(p.lanes.filter_a.lp_hz.sample(n), 20000.);
        close(p.lanes.rate_b.sample(n), 1.);
        assert!(p.duration_sec() < 1.); // transport settling, no musical pre-roll
    }
}

fn structured(
    break_bars: u32,
) -> (
    mixless_protocol::TrackAnalysis,
    mixless_protocol::TrackAnalysis,
) {
    let mut a = track(1, 174., "8A", S::Outro, 128, 0.8, 0.2);
    let mut b = track(2, 174., "9A", S::Intro, 128, 0.8, 0.2);
    let sec = 240. / 174.;
    a.sections = [
        (0, 16, S::Intro),
        (16, 32, S::Drop),
        (32, 48, S::Drop),
        (48, 48 + break_bars, S::Break),
        (48 + break_bars, 112, S::Drop),
        (112, 128, S::Outro),
    ]
    .into_iter()
    .map(|(a, b, label)| Section {
        start_sec: a as f32 * sec,
        end_sec: b as f32 * sec,
        label,
    })
    .collect();
    for bar in &mut a.bars {
        bar.section = a
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec - 0.001 && bar.start_sec < s.end_sec - 0.001)
            .unwrap()
            .label;
    }
    measured_phrases(
        &mut a,
        &[
            0,
            8,
            16,
            24,
            32,
            40,
            48,
            48 + break_bars / 2,
            48 + break_bars,
            112,
            120,
            128,
        ],
    );
    measured_phrases(&mut b, &[0, break_bars / 2, break_bars, 112, 128]);
    (a, b)
}

#[test]
fn recovery_overlap_preserves_both_halves_of_drop_and_adapts_to_available_phrases() {
    let mut lengths = std::collections::BTreeSet::new();
    for bars in [8, 16, 24] {
        let (a, b) = structured(bars);
        let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        let summary = p.summary.as_ref().expect("recovery blend");
        // The overlap may sit inside the break between the drops or ride a
        // drop's tail to its measured end; it must never throw away the next
        // drop's opening downbeat.
        let in_break =
            p.t_in_a + 0.01 >= a.sections[2].end_sec && p.t_out_a <= a.sections[3].end_sec + 0.01;
        let on_peak_end = crate::drops::peaks(&a)
            .iter()
            .any(|&(_, end)| (p.t_out_a - end).abs() < 0.05);
        assert!(
            in_break || on_peak_end,
            "{summary:?}, window {}-{}",
            p.t_in_a,
            p.t_out_a
        );
        // Finishing on a matching incoming phrase may use part of the break.
        // Require a real overlap, bounded by the available recovery interval.
        assert!(
            (4..=bars as u16).contains(&summary.length_bars),
            "{summary:?}"
        );
        lengths.insert(summary.length_bars);
        assert_eq!(p.incoming_start_bar, 0.);
        let n = summary.length_bars as f32;
        assert!(p.lanes.xfader.sample(n * 0.5).abs() < 0.9);
        assert!(p.lanes.gain_a.sample(n * 0.5) > -12. && p.lanes.gain_b.sample(n * 0.5) > -12.);
    }
    assert!(lengths.len() >= 2, "every recovery used {lengths:?}");
}

#[test]
fn later_or_partially_entered_drop_cannot_be_faded_halfway_through() {
    let (a, _) = structured(16);
    assert!(!crate::drops::allows(&a, 24., 42., 0., false));
    assert!(crate::drops::allows(
        &a,
        48. * 240. / 174. - 8.,
        48. * 240. / 174.,
        0.,
        false
    ));
    assert!(!crate::drops::allows(&a, 90., 110., 0., false));
    assert!(!crate::drops::allows(&a, 80., 90., 30., true));
}

fn melodic_pair(
    b_drop_bar: u32,
) -> (
    mixless_protocol::TrackAnalysis,
    mixless_protocol::TrackAnalysis,
) {
    let bar = 240. / 128.;
    let mut a = track(1, 128., "8A", S::Outro, 128, 0.8, 0.2);
    a.sections = [
        (0, 16, S::Intro),
        (16, 32, S::BuildUp),
        (32, 64, S::Drop),
        (64, 80, S::Break),
        (80, 112, S::Drop),
        (112, 128, S::Outro),
    ]
    .into_iter()
    .map(|(x, y, label)| Section {
        start_sec: x as f32 * bar,
        end_sec: y as f32 * bar,
        label,
    })
    .collect();
    for bar in &mut a.bars {
        bar.section = a
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec && bar.start_sec < s.end_sec)
            .unwrap()
            .label;
    }
    let mut b = track(2, 128., "9A", S::Intro, 64, 0.8, 0.2);
    b.sections = [
        (0, b_drop_bar, S::Intro),
        (b_drop_bar, 48, S::Drop),
        (48, 64, S::Outro),
    ]
    .into_iter()
    .map(|(x, y, label)| Section {
        start_sec: x as f32 * bar,
        end_sec: y as f32 * bar,
        label,
    })
    .collect();
    for bar in &mut b.bars {
        bar.section = b
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec && bar.start_sec < s.end_sec)
            .unwrap()
            .label;
    }
    (a, b)
}

fn assert_exit_respects_peaks(
    p: &mixless_protocol::MixPlan,
    a: &mixless_protocol::TrackAnalysis,
    b: &mixless_protocol::TrackAnalysis,
) {
    let layered = matches!(
        p.transition_mode,
        Some(mixless_protocol::TransitionMode::BeatBlend)
            | Some(mixless_protocol::TransitionMode::LoopRoll)
    );
    let lands = if layered { p.t_end_b } else { p.t_in_b };
    for &(start, _) in &crate::drops::peaks(a) {
        let into_drop = (p.t_out_a - start).abs() < 0.05
            || a.sections.iter().any(|s| {
                s.label == S::BuildUp
                    && p.t_out_a > s.start_sec + 0.08
                    && p.t_out_a <= s.end_sec + 0.08
                    && s.end_sec <= start + 0.08
            });
        if into_drop {
            assert!(
                crate::drops::peaks(b)
                    .iter()
                    .any(|q| (q.0 - lands).abs() < 0.08),
                "exit {:.2} spends A's build into the drop at {:.2} without B \
                 peaking at the landing {:.2}",
                p.t_out_a,
                start,
                lands
            );
        }
    }
    if layered {
        for &(start, _) in &crate::drops::peaks(b) {
            assert!(
                !(start >= p.t_in_b - 0.05 && start < p.t_end_b - 0.05),
                "incoming peak at {start:.2} starts inside the overlap \
                 {:.2}-{:.2}",
                p.t_in_b,
                p.t_end_b
            );
        }
    }
}

#[test]
fn exit_before_outgoing_drop_requires_incoming_drop_at_landing() {
    // B's drop arrives too late to resolve A's build at the landing point.
    let (a, b) = melodic_pair(32);
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.summary.is_some(), "{:?}", p.failure_reason);
    assert_exit_respects_peaks(&p, &a, &b);
    // With an earlier incoming drop a build-up swap is available.
    let (a, b) = melodic_pair(16);
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.summary.is_some(), "{:?}", p.failure_reason);
    assert_exit_respects_peaks(&p, &a, &b);
}

#[test]
fn filtered_blend_is_capped_at_sixteen_bars() {
    // Incompatible keys with clean, vocal-free drum evidence on both sides
    // used to let a filter sweep layer two full tracks for tens of bars.
    let mut a = track(1, 128., "8A", S::Outro, 96, 0.8, 0.2);
    let mut b = track(2, 128., "3B", S::Intro, 96, 0.8, 0.2);
    for bar in a.bars.iter_mut().chain(&mut b.bars) {
        bar.vocal_confidence = Some(0.1);
    }
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let summary = p.summary.as_ref().expect("plan: {p:?}");
    if summary.strategy == StrategyId::FilterSweep {
        assert!(
            summary.length_bars <= 16,
            "filtered blend ran {} bars",
            summary.length_bars
        );
    }
}

#[test]
fn short_or_quiet_first_chorus_is_not_the_main_peak() {
    let bar = 240. / 120.;
    let mut t = track(1, 120., "8A", S::Outro, 96, 0.8, 0.2);
    t.sections = [
        (0, 16, S::Intro),
        (16, 18, S::Drop), // a two-bar fill, not a peak
        (18, 24, S::Break),
        (24, 32, S::Chorus), // lighter first chorus
        (32, 48, S::Break),
        (48, 80, S::Drop), // the main drop
        (80, 96, S::Outro),
    ]
    .into_iter()
    .map(|(x, y, label)| Section {
        start_sec: x as f32 * bar,
        end_sec: y as f32 * bar,
        label,
    })
    .collect();
    for bar in &mut t.bars {
        let s = t
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec && bar.start_sec < s.end_sec)
            .unwrap();
        bar.section = s.label;
        bar.rms = match s.label {
            S::Chorus => 0.16, // 0.8x the drop's level: not a major peak
            S::Drop => 0.2,
            _ => 0.1,
        };
    }
    let peaks = crate::drops::peaks(&t);
    assert_eq!(peaks.len(), 2, "two-bar fill counted as a peak: {peaks:?}");
    let major = crate::drops::major_peaks(&t);
    assert_eq!(major.len(), 1, "quiet first chorus counted: {major:?}");
    assert!((major[0].0 - 48. * bar).abs() < 0.01);
    assert!(
        crate::drops::penalty(&t, 40. * bar, 0.) > 0.,
        "exit after the light chorus is not free"
    );
    assert_eq!(
        crate::drops::next_peak_start(&t, 33. * bar),
        Some(48. * bar)
    );
}

#[test]
fn terminal_drop_fallback_holds_to_the_end_before_launching_incoming() {
    let (mut a, b) = structured(16);
    a.sections = vec![
        a.sections[0].clone(),
        Section {
            start_sec: 16. * 240. / 174.,
            end_sec: a.duration_sec,
            label: S::Drop,
        },
    ];
    let ctx = crate::PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let p = crate::short_handoff(&ctx, a.duration_sec - 3.);
    assert!(p.failure_reason.is_none());
    assert_eq!(p.summary.as_ref().unwrap().strategy, StrategyId::DryCut);
    close(p.t_out_a, a.duration_sec);
    close(p.incoming_start_bar, 1.);
    close(p.lanes.xfader.sample(0.95), -1.);
}

fn spinback_pair(
    id_a: i64,
    id_b: i64,
    b_drop_bar: Option<u32>,
) -> (
    mixless_protocol::TrackAnalysis,
    mixless_protocol::TrackAnalysis,
) {
    spinback_pair_at(id_a, id_b, b_drop_bar, 128.)
}

fn spinback_pair_at(
    id_a: i64,
    id_b: i64,
    b_drop_bar: Option<u32>,
    bpm_b: f32,
) -> (
    mixless_protocol::TrackAnalysis,
    mixless_protocol::TrackAnalysis,
) {
    let bar = 240. / 128.;
    let mut a = track(id_a, 128., "8A", S::Drop, 128, 0.9, 0.2);
    a.sections = [
        (0, 16, S::Intro),
        (16, 32, S::BuildUp),
        (32, 64, S::Drop),
        (64, 80, S::Break),
        (80, 112, S::Drop),
        (112, 128, S::Outro),
    ]
    .into_iter()
    .map(|(x, y, label)| Section {
        start_sec: x as f32 * bar,
        end_sec: y as f32 * bar,
        label,
    })
    .collect();
    for bar in &mut a.bars {
        bar.section = a
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec && bar.start_sec < s.end_sec)
            .unwrap()
            .label;
    }
    let bar_b = 240. / bpm_b;
    let mut b = track(id_b, bpm_b, "9A", S::Intro, 96, 0.9, 0.2);
    b.sections = match b_drop_bar {
        Some(drop) => [(0, drop, S::Intro), (drop, 64, S::Drop), (64, 96, S::Outro)]
            .into_iter()
            .map(|(x, y, label)| Section {
                start_sec: x as f32 * bar_b,
                end_sec: y as f32 * bar_b,
                label,
            })
            .collect(),
        None => vec![Section {
            start_sec: 0.,
            end_sec: 96. * bar_b,
            label: S::Intro,
        }],
    };
    for bar in &mut b.bars {
        bar.section = b
            .sections
            .iter()
            .find(|s| bar.start_sec >= s.start_sec && bar.start_sec < s.end_sec)
            .unwrap()
            .label;
    }
    (a, b)
}

#[test]
fn spinback_needs_incoming_drop_landing_and_clean_outgoing_tail() {
    let (a, b) = spinback_pair(1, 2, Some(32));
    let p = Planner::with_options(PlannerOptions {
        strategy: Some(StrategyId::Spinback),
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let summary = p
        .summary
        .as_ref()
        .unwrap_or_else(|| panic!("spinback plan: {:?}", p.failure_reason));
    assert_eq!(summary.strategy, StrategyId::Spinback);
    assert!(!summary.used_fallback);
    let op = p.lanes.scratch_a.as_ref().expect("backspin scratch op");
    assert!(op.accelerate);
    assert!(op.peak_delta_frames < 0);
    let n = summary.length_bars as f32;
    assert!(op.on_bar >= n - 0.5 && op.off_bar <= n);
    close(p.lanes.gain_a.sample(n), -96.);
    close(p.incoming_start_bar, n);
    // The spin releases exactly onto B's drop downbeat.
    close(p.t_in_b, 32. * 240. / 128.);
    // Without an incoming drop at the landing there is no legal spinback.
    let (a, intro_b) = spinback_pair(1, 2, None);
    let refused = Planner::with_options(PlannerOptions {
        strategy: Some(StrategyId::Spinback),
        ..Default::default()
    })
    .plan_pair(
        &a,
        &intro_b,
        &[],
        &[],
        Default::default(),
        Default::default(),
    );
    assert!(refused.summary.is_none());
    assert!(refused.failure_reason.is_some());
}

#[test]
fn spinback_is_never_chosen_on_a_sung_tail() {
    let (mut a, b) = spinback_pair(1, 2, Some(32));
    for bar in &mut a.bars {
        bar.vocal_presence = 0.9;
    }
    let p = Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(p.lanes.scratch_a.is_none());
    if let Some(summary) = &p.summary {
        assert_ne!(summary.strategy, StrategyId::Spinback);
    }
    let forced = Planner::with_options(PlannerOptions {
        strategy: Some(StrategyId::Spinback),
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    assert!(forced.summary.is_none());
}

#[test]
fn loop_out_loops_the_outgoing_phrase_and_hands_over_on_the_bass() {
    let a = crate::stem_regressions::with_stems(track(1, 128., "8A", S::Drop, 64, 0.8, 0.2));
    let b = crate::stem_regressions::with_stems(track(2, 128., "9A", S::Intro, 64, 0.8, 0.2));
    let p = Planner::with_options(PlannerOptions {
        strategy: Some(StrategyId::LoopOut),
        stem_playback: true,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let summary = p
        .summary
        .as_ref()
        .unwrap_or_else(|| panic!("loop out plan: {:?}", p.failure_reason));
    assert_eq!(summary.strategy, StrategyId::LoopOut);
    assert!(!summary.used_fallback);
    let n = summary.length_bars as f32;
    assert!(
        n == 4. || n == 8.,
        "loop out uses a short blend window, n={n}"
    );
    let op = p.lanes.loop_a.as_ref().expect("outgoing phrase loop");
    close(op.on_bar, 0.);
    close(op.off_bar, n);
    assert_eq!(op.length_bars, if n >= 8. { 2 } else { 1 });
    assert!(p.requires_stems);
    let mix = p.stem_mix.as_ref().unwrap();
    close(
        mix.outgoing[mixless_protocol::StemKind::Vocals.index()].sample(0.25),
        0.,
    );
    close(
        mix.incoming[mixless_protocol::StemKind::Drums.index()].sample(0.),
        1.,
    );
    close(p.incoming_start_bar, 0.);
    close(p.lanes.xfader.sample(n), 1.);
    // Without stem buffers a sung loop window cannot be stripped safely.
    let mut sung = track(1, 128., "8A", S::Drop, 64, 0.8, 0.2);
    for bar in &mut sung.bars {
        bar.vocal_presence = 0.9;
    }
    let plain = track(2, 128., "9A", S::Intro, 64, 0.8, 0.2);
    let refused = Planner::with_options(PlannerOptions {
        strategy: Some(StrategyId::LoopOut),
        ..Default::default()
    })
    .plan_pair(
        &sung,
        &plain,
        &[],
        &[],
        Default::default(),
        Default::default(),
    );
    assert!(refused.summary.is_none());
    // The EQ fallback still applies on an instrumental tail.
    let p = Planner::with_options(PlannerOptions {
        strategy: Some(StrategyId::LoopOut),
        ..Default::default()
    })
    .plan_pair(
        &track(1, 128., "8A", S::Drop, 64, 0.8, 0.2),
        &plain,
        &[],
        &[],
        Default::default(),
        Default::default(),
    );
    let summary = p
        .summary
        .as_ref()
        .unwrap_or_else(|| panic!("EQ loop out plan: {:?}", p.failure_reason));
    assert_eq!(summary.strategy, StrategyId::LoopOut);
    assert!(!p.requires_stems);
    let n = summary.length_bars as f32;
    close(p.lanes.eq_a.mid.sample(n / 2.), -12.);
    close(p.lanes.eq_a.high.sample(n / 2.), -6.);
}

#[test]
fn technique_variety_is_deterministic() {
    // Identical inputs must produce an identical plan — variety is a hash of
    // the pair and the exit, never a random choice.
    let (a, b) = spinback_pair(1, 2, Some(32));
    let first = Planner::with_options(PlannerOptions {
        live_moves: crate::LiveMoves::Active,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let second = Planner::with_options(PlannerOptions {
        live_moves: crate::LiveMoves::Active,
        ..Default::default()
    })
    .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
    let first = first.summary.expect("first plan").strategy;
    assert_eq!(first, second.summary.expect("repeat plan").strategy);
    // The same fixture family with different track ids must rotate between
    // the valid techniques — the spinback is an accent, not a default. A
    // mismatched tempo leaves only the bridge techniques to choose between.
    let mut strategies = std::collections::HashSet::new();
    for id in (1..20).step_by(2) {
        let (a, b) = spinback_pair_at(id, id + 1, Some(32), 100.);
        let p = Planner::with_options(PlannerOptions {
            live_moves: crate::LiveMoves::Active,
            ..Default::default()
        })
        .plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        let subtle =
            Planner::new().plan_pair(&a, &b, &[], &[], Default::default(), Default::default());
        assert_ne!(
            subtle.summary.as_ref().map(|s| s.strategy),
            Some(StrategyId::Spinback)
        );
        strategies.insert(
            p.summary
                .unwrap_or_else(|| panic!("pair {id}/{}: {:?}", id + 1, p.failure_reason))
                .strategy,
        );
    }
    assert!(
        strategies.len() >= 2,
        "rotation produced only {strategies:?}"
    );
    assert!(
        strategies.contains(&StrategyId::Spinback),
        "spinback never fired: {strategies:?}"
    );
}
