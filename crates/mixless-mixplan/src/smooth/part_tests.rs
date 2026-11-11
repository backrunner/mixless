use super::*;
use mixless_protocol::{SectionLabel, StemKind, StemNote};

#[test]
fn muted_entry_can_wait_for_a_complete_vocal_rest_but_full_track_entry_cannot() {
    let mut a = crate::stem_regressions::with_stems(crate::tests::track(
        1,
        120.,
        "8A",
        SectionLabel::Outro,
        64,
        0.9,
        0.3,
    ));
    let mut b = crate::stem_regressions::with_stems(crate::tests::track(
        2,
        100.,
        "8A",
        SectionLabel::Intro,
        64,
        0.9,
        0.3,
    ));
    for t in [&mut a, &mut b] {
        for f in &mut t.stems.as_mut().unwrap().frames {
            f.vocal_activity = 0.;
        }
        for bar in &mut t.bars {
            bar.vocal_presence = 0.;
            bar.vocal_confidence = Some(0.);
        }
    }
    let entry = Grid(&b).sec(8.);
    for (t, time_sec) in [(&mut a, 28.), (&mut b, entry)] {
        t.phrase_boundaries = vec![mixless_protocol::PhraseBoundary {
            time_sec,
            confidence: 0.9,
            novelty: 0.5,
        }];
    }
    for f in &mut b.stems.as_mut().unwrap().frames {
        if f.start_sec < entry + 3. {
            f.vocal_activity = 1.;
        }
    }
    b.stems.as_mut().unwrap().notes.push(StemNote {
        stem: StemKind::Vocals,
        start_sec: entry - 1.,
        end_sec: entry + 3.,
        midi: 60,
        confidence: 0.9,
    });
    assert!(!crate::vocals::safe_entry(&b, entry));
    let run = |b: &TrackAnalysis, stems, minimum_score| {
        candidate::direct(
            &PlanContext {
                outgoing: &a,
                incoming: b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            },
            &PlannerOptions {
                stem_playback: stems,
                ..Default::default()
            },
            56.,
            8.,
            TransitionMode::PhraseBridge,
            8,
            minimum_score,
            false,
        )
    };
    assert!(run(&b, false, -1.).is_none());
    let p = run(&b, true, -1.).expect("fit a native-tempo bed with delayed vocal entry");
    assert!(
        run(&b, true, p.summary.as_ref().unwrap().score + 0.00001).is_none(),
        "late energy/residence deductions must respect the incumbent's final score"
    );
    assert!(p.requires_stems);
    let voice = &p.stem_mix.as_ref().unwrap().incoming[0];
    for i in 0..=128 {
        let u = 8. * i as f32 / 128.;
        if p.incoming_source.sample(u) < entry + 3. {
            assert!(voice.sample(u) < 0.001, "entered a held word at {u}");
        }
    }
    for f in &mut b.stems.as_mut().unwrap().frames {
        f.vocal_activity = 1.;
    }
    b.stems.as_mut().unwrap().notes[0].end_sec = b.duration_sec;
    assert!(
        run(&b, true, -1.).is_none(),
        "no complete rest means no fitted entry"
    );
}
