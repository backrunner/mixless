use super::*;
use mixless_protocol::{Cue, CueKind, SectionLabel};

fn serial(ctx: &PlanContext<'_>, options: &PlannerOptions, outs: &[f32], ins: &[f32]) -> Scored {
    let mut best: Scored = None;
    for &out in outs {
        for &input in ins {
            for mode in [
                TransitionMode::BeatBlend,
                TransitionMode::LoopRoll,
                TransitionMode::PhraseBridge,
            ] {
                for length in crate::duration::candidates(ctx, options, out, input, mode) {
                    if let Some(p) = super::super::pair(
                        ctx,
                        options,
                        out,
                        input,
                        mode,
                        length,
                        best.as_ref().map_or(-1., |p| p.0),
                    ) {
                        let merit = crate::experience::merit(&p, ctx.outgoing, ctx.incoming);
                        if best.as_ref().is_none_or(|old| merit > old.0) {
                            best = Some((merit, p));
                        }
                    }
                }
            }
        }
    }
    best
}

#[test]
fn ordered_batches_preserve_full_plans_with_stems_cues_offsets_and_missing_grid() {
    let mut successful = 0;
    let mut failed = 0;
    for (bpm, key, stems, manual, weak) in [
        (120., "8A", false, false, false),
        (128., "8A", true, false, false),
        (100., "8A", true, false, false),
        (120., "3A", true, false, false),
        (120., "8A", true, true, false),
        (120., "8A", false, false, true),
    ] {
        let mut a = crate::stem_regressions::with_stems(crate::tests::track(
            1,
            120.,
            "8A",
            SectionLabel::Outro,
            64,
            0.8,
            0.2,
        ));
        let mut b = crate::stem_regressions::with_stems(crate::tests::track(
            2,
            bpm,
            key,
            SectionLabel::Intro,
            64,
            0.8,
            0.2,
        ));
        if weak {
            a.tempo.beats.clear();
            b.tempo.beats.clear();
        }
        let out_cue = [Cue {
            index: 0,
            frame: 48 * a.sample_rate as u64,
            kind: CueKind::Out,
            user_set: true,
        }];
        let in_cue = [Cue {
            index: 0,
            frame: 4 * b.sample_rate as u64,
            kind: CueKind::In,
            user_set: true,
        }];
        let ctx = PlanContext {
            outgoing: &a,
            incoming: &b,
            cues_out: if manual { &out_cue } else { &[] },
            cues_in: if manual { &in_cue } else { &[] },
            offset_a: mixless_protocol::PerformanceOffset {
                rate: if manual { 1.04 } else { 1. },
                pitch_semitones: 0.,
            },
            offset_b: Default::default(),
        };
        let options = PlannerOptions {
            stem_playback: stems,
            harmonic_key_shift: true,
            ..Default::default()
        };
        let outs = crate::phrasing::points(&a, ctx.cues_out, true, 0.);
        let ins = crate::phrasing::points(&b, ctx.cues_in, false, 0.);
        let evidence = crate::evidence::Prepared::new(&ctx, stems);
        let expected = evidence.run(|| serial(&ctx, &options, &outs, &ins));
        if expected.is_some() {
            successful += 1;
        } else {
            failed += 1;
        }
        for batch in [1, 3, 36] {
            let result = evidence.run(|| single_with_batch(&ctx, &options, &outs, &ins, batch));
            assert_eq!(
                serde_json::to_value(&result).unwrap(),
                serde_json::to_value(&expected).unwrap(),
                "bpm={bpm} key={key} stems={stems} manual={manual} weak={weak} batch={batch}"
            );
        }
    }
    assert!(
        successful > 0 && failed > 0,
        "cover successful and rejected searches"
    );
}

#[test]
fn equal_scores_keep_the_first_anchor_and_empty_search_stays_empty() {
    let a = crate::tests::track(1, 120., "8A", SectionLabel::Outro, 64, 0.8, 0.2);
    let b = crate::tests::track(2, 120., "8A", SectionLabel::Intro, 64, 0.8, 0.2);
    let ctx = PlanContext {
        outgoing: &a,
        incoming: &b,
        cues_out: &[],
        cues_in: &[],
        offset_a: Default::default(),
        offset_b: Default::default(),
    };
    let opts = PlannerOptions::default();
    let outs = [256., 224., 256., 224.];
    let ins = [0., 32., 0., 32.];
    let expected = serial(&ctx, &opts, &outs, &ins);
    assert!(expected.is_some());
    for _ in 0..3 {
        let actual = single_with_batch(&ctx, &opts, &outs, &ins, 8);
        assert_eq!(
            serde_json::to_value(&actual).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }
    assert!(single_with_batch(&ctx, &opts, &[], &ins, 8).is_none());
    assert!(single_with_batch(&ctx, &opts, &outs, &[], 8).is_none());
}
