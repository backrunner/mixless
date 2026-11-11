use super::*;
use mixless_protocol::{
    Cue, CueKind, LiveMoves, PerformanceOffset, SectionLabel, StrategyId, WhoStretches,
};

fn track(id: i64) -> TrackAnalysis {
    crate::tests::track(id, 120., "8A", SectionLabel::Intro, 32, 0.8, 0.2)
}
fn context<'a>(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> PlanContext<'a> {
    PlanContext {
        outgoing: a,
        incoming: b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    }
}
fn result(value: f32) -> Vec<MixPlan> {
    vec![MixPlan {
        t_in_a: value,
        ..Default::default()
    }]
}

#[test]
fn keys_cover_all_options_cues_offsets_and_shortlist_mode() {
    let a = track(1);
    let b = track(2);
    let mut ctx = context(&a, &b);
    let options = PlannerOptions::default();
    let original = Key::new(&ctx, &options, false);
    assert_ne!(original, Key::new(&ctx, &options, true));
    let mut variants = Vec::new();
    for i in 0..9 {
        let mut o = options.clone();
        match i {
            0 => o.harmonic_key_shift = true,
            1 => o.smooth = false,
            2 => o.who_stretches = WhoStretches::A,
            3 => o.literal_half_double = true,
            4 => o.strategy = Some(StrategyId::EnergyHold),
            5 => o.earliest_outgoing_sec = f32::from_bits(1),
            6 => o.outgoing_entry_sec = -0.,
            7 => o.stem_playback = true,
            _ => o.live_moves = LiveMoves::Active,
        }
        variants.push(o);
    }
    for o in variants {
        assert_ne!(original, Key::new(&ctx, &o, false));
    }
    for i in 0..4 {
        ctx.offset_a = PerformanceOffset::identity();
        ctx.offset_b = PerformanceOffset::identity();
        match i {
            0 => ctx.offset_a.rate = f32::from_bits(1f32.to_bits() + 1),
            1 => ctx.offset_a.pitch_semitones = -0.,
            2 => ctx.offset_b.rate = 1.01,
            _ => ctx.offset_b.pitch_semitones = 1.,
        }
        assert_ne!(original, Key::new(&ctx, &options, false));
    }
    ctx = context(&a, &b);
    let cue = Cue {
        index: 0,
        frame: 10,
        kind: CueKind::In,
        user_set: true,
    };
    let baseline_cues = [cue.clone()];
    ctx.cues_in = &baseline_cues;
    let original_cue = Key::new(&ctx, &options, false);
    for i in 0..4 {
        let mut changed = cue.clone();
        match i {
            0 => changed.index += 1,
            1 => changed.frame += 1,
            2 => changed.kind = CueKind::Out,
            _ => changed.user_set = false,
        }
        let cues = [changed];
        let c = PlanContext {
            cues_in: &cues,
            ..context(&a, &b)
        };
        assert_ne!(original_cue, Key::new(&c, &options, false));
    }
    let c = PlanContext {
        cues_out: &baseline_cues,
        ..context(&a, &b)
    };
    assert_ne!(original_cue, Key::new(&c, &options, false));
}

#[test]
#[cfg(not(feature = "trace"))]
fn completed_plans_and_failures_are_reused_but_callers_own_their_copies() {
    let a = track(1);
    let b = track(2);
    let options = PlannerOptions::default();
    let session = Session::new(&[&a, &b]);
    let _scope = session.enter();
    let ctx = context(&a, &b);
    let mut first = plans(&ctx, &options, false, || result(3.));
    first[0].t_in_a = 9.;
    assert_eq!(
        plans(&ctx, &options, false, || panic!("cache miss"))[0].t_in_a,
        3.
    );
    let failed = || {
        vec![MixPlan {
            failure_reason: Some("No safe window".into()),
            ..Default::default()
        }]
    };
    plans(&ctx, &options, true, failed);
    assert_eq!(
        plans(&ctx, &options, true, || panic!("failed plans are reusable"))[0]
            .failure_reason
            .as_deref(),
        Some("No safe window")
    );
}

#[test]
#[cfg(not(feature = "trace"))]
fn workers_share_completed_values_and_restore_the_parent() {
    let a = track(1);
    let b = track(2);
    let foreign = track(1);
    let options = PlannerOptions::default();
    let session = Session::new(&[&a, &b]);
    let _scope = session.enter();
    plans(&context(&a, &b), &options, false, || result(1.));
    crate::order::parallel::map(32, |_| {
        let nested = Session::new(&[&foreign, &b]);
        {
            let _nested = nested.enter();
            assert_eq!(
                plans(&context(&foreign, &b), &options, false, || result(2.))[0].t_in_a,
                2.
            );
            // Foreign same-ID analyses cannot read or populate the parent's entry.
            assert_eq!(
                plans(&context(&a, &b), &options, false, || result(4.))[0].t_in_a,
                4.
            );
        }
        crate::order::parallel::map(2, |_| {
            assert_eq!(
                plans(&context(&a, &b), &options, false, || panic!(
                    "parent cache lost"
                ))[0]
                    .t_in_a,
                1.
            );
        });
    });
    assert_eq!(session.shared.values.lock().unwrap().hits, 64);
}

#[test]
#[cfg(not(feature = "trace"))]
fn independent_requests_cannot_inherit_edited_evidence() {
    let mut a = track(1);
    let b = track(2);
    let options = PlannerOptions::default();
    {
        let session = Session::new(&[&a, &b]);
        let _scope = session.enter();
        plans(&context(&a, &b), &options, false, || result(1.));
    }
    assert!(CURRENT.with(|c| c.borrow().is_none()));
    a.duration_sec += 1.;
    let session = Session::new(&[&a, &b]);
    let _scope = session.enter();
    assert_eq!(
        plans(&context(&a, &b), &options, false, || result(2.))[0].t_in_a,
        2.
    );
}

#[test]
#[cfg(not(feature = "trace"))]
fn eviction_recomputes_without_truncating_search_and_oversized_plans_bypass() {
    let a = track(1);
    let b = track(2);
    let ctx = context(&a, &b);
    let mut session = Session::new(&[&a, &b]);
    let limits = Limits {
        entries: 8,
        bytes: 128 * 1024,
    };
    Arc::get_mut(&mut session.shared).unwrap().limits = limits;
    let _scope = session.enter();
    let mut options = PlannerOptions::default();
    for i in 0..limits.entries + 1 {
        options.earliest_outgoing_sec = i as f32;
        plans(&ctx, &options, false, || result(i as f32));
    }
    options.earliest_outgoing_sec = 0.;
    assert_eq!(
        plans(&ctx, &options, false, || result(999.))[0].t_in_a,
        999.
    );
    let values = session.shared.values.lock().unwrap();
    assert_eq!(values.entries.len(), limits.entries);
    assert_eq!(values.evictions, 2);
    assert!(values.bytes <= limits.bytes);
    drop(values);
    let mut huge = MixPlan::default();
    huge.clock.nodes = vec![(0., 1.); limits.bytes / 8 + 1];
    plans(&ctx, &options, true, || vec![huge]);
    assert_eq!(plans(&ctx, &options, true, || result(7.))[0].t_in_a, 7.);
}

#[test]
#[cfg(feature = "trace")]
fn trace_never_skips_the_search_collectors() {
    let a = track(1);
    let b = track(2);
    let options = PlannerOptions::default();
    let session = Session::new(&[&a, &b]);
    let _scope = session.enter();
    for value in [1., 2.] {
        assert_eq!(
            plans(&context(&a, &b), &options, false, || result(value))[0].t_in_a,
            value
        );
    }
}

#[test]
fn request_budgets_scale_without_unbounded_allocation() {
    let small = Limits::for_tracks(2);
    let large = Limits::for_tracks(29);
    assert_eq!(small.entries, 256);
    assert_eq!(small.bytes, 64 * 1024 * 1024);
    assert!(large.entries >= 29 * 28);
    assert!(large.bytes > small.bytes);
    let huge = Limits::for_tracks(usize::MAX);
    assert_eq!(huge.entries, 8192);
    assert_eq!(huge.bytes, 512 * 1024 * 1024);
}
