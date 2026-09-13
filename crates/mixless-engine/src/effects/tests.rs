//! Catalogue-wide audio behavior and callback budget regression coverage.

use super::*;

#[test]
fn every_effect_is_dry_at_zero_mix_and_when_bypassed() {
    for kind in [
        EffectKind::Echo,
        EffectKind::Flanger,
        EffectKind::Gate,
        EffectKind::Reverb,
        EffectKind::Phaser,
        EffectKind::Chorus,
        EffectKind::Tremolo,
        EffectKind::Filter,
        EffectKind::Crush,
        EffectKind::Dist,
        EffectKind::Noise,
    ] {
        for bypass in [true, false] {
            let mut effect = Effect::new(48_000.0);
            effect.configure(EffectParams {
                kind,
                mix: 0.0,
                bypass,
                ..EffectParams::default()
            });
            for frame in 0..1200 {
                let input = [(frame as f32 * 0.17).sin() * 0.2, -0.1];
                assert_eq!(effect.process(input, false), input, "{kind:?}");
            }
        }
    }
}

#[test]
fn echo_is_tempo_synced_and_send_is_wet_only() {
    for sample_rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut effect = Effect::new(sample_rate);
        effect.configure(EffectParams {
            mix: 1.0,
            bypass: false,
            feedback: 0.5,
            ..EffectParams::default()
        });
        for _ in 0..2000 {
            effect.process([0.0; 2], true);
        }
        let delay = (sample_rate * 0.25).round() as usize;
        assert_eq!(effect.process([0.5, 0.0], true), [0.0; 2]);
        for _ in 1..delay {
            assert_eq!(effect.process([0.0; 2], true), [0.0; 2]);
        }
        assert!((effect.process([0.0; 2], true)[0] - 0.5).abs() < 0.001);
        for _ in 1..delay {
            effect.process([0.0; 2], true);
        }
        assert!((effect.process([0.0; 2], true)[0] - 0.25).abs() < 0.001);
    }
}

#[test]
fn effects_have_distinct_audible_output_and_bounded_release() {
    for kind in [
        EffectKind::Echo,
        EffectKind::Flanger,
        EffectKind::Gate,
        EffectKind::Reverb,
        EffectKind::Phaser,
        EffectKind::Chorus,
        EffectKind::Tremolo,
        EffectKind::Filter,
        EffectKind::Crush,
        EffectKind::Dist,
        EffectKind::Noise,
    ] {
        let mut effect = Effect::new(48_000.0);
        effect.configure(EffectParams {
            kind,
            mix: 0.7,
            bypass: false,
            ..EffectParams::default()
        });
        let mut difference = 0.0;
        for frame in 0..48_000 {
            let input = [
                (frame as f32 * 0.123).sin() * 0.1,
                (frame as f32 * 0.19).sin() * 0.1,
            ];
            let output = effect.process(input, false);
            assert!(output
                .iter()
                .all(|sample| sample.is_finite() && sample.abs() < 2.0));
            difference += (input[0] - output[0]).abs();
        }
        assert!(difference > 1.0, "{kind:?} appears bypassed");
        effect.configure(EffectParams {
            kind,
            bypass: true,
            ..EffectParams::default()
        });
        for _ in 0..8000 {
            effect.process([0.1, -0.1], false);
        }
        assert_eq!(effect.process([0.1, -0.1], false), [0.1, -0.1]);
    }
}

#[test]
fn reverb_tail_decays_without_input() {
    let mut effect = Effect::new(48_000.0);
    effect.configure(EffectParams {
        kind: EffectKind::Reverb,
        mix: 1.0,
        bypass: false,
        ..EffectParams::default()
    });
    for _ in 0..1000 {
        effect.process([0.0; 2], true);
    }
    effect.process([1.0, 0.0], true);
    let mut early = 0.0;
    let mut late = 0.0;
    for frame in 0..144_000 {
        let output = effect.process([0.0; 2], true);
        let energy = output[0] * output[0] + output[1] * output[1];
        if frame < 48_000 {
            early += energy;
        }
        if frame >= 96_000 {
            late += energy;
        }
    }
    assert!(early > 0.01 && late < early * 0.01, "{early} -> {late}");
}

#[test]
fn complete_catalogue_is_finite_and_audible() {
    for kind in EffectKind::ALL {
        let mut effect = Effect::new(48_000.0);
        effect.configure(EffectParams {
            kind,
            mix: 0.8,
            bypass: false,
            feedback: 0.65,
            depth: 0.7,
            drive: 0.7,
            ..EffectParams::default()
        });
        let mut energy = 0.0;
        for frame in 0..48_000 {
            let input = [
                (frame as f32 * 0.113).sin() * 0.2,
                (frame as f32 * 0.173).cos() * 0.15,
            ];
            let output = effect.process(input, false);
            assert!(
                output
                    .iter()
                    .all(|sample| sample.is_finite() && sample.abs() < 2.0),
                "{kind:?}"
            );
            energy += (output[0] - input[0]).abs() + (output[1] - input[1]).abs();
        }
        // Dynamics effects can be intentionally transparent below their
        // threshold; the finite-output assertion above is the invariant.
        assert!(energy > 0.001, "{kind:?} appears bypassed");
    }
}
#[test]
fn freeze_preserves_room_energy_and_release_closes_injection() {
    let mut freeze = Effect::new(48000.0);
    let p = EffectParams {
        kind: EffectKind::FreezeVerb,
        mix: 1.0,
        bypass: false,
        beats: 0.5,
        decay_seconds: 1.6,
        ..EffectParams::default()
    };
    freeze.configure(p);
    let mut early = 0.0;
    let mut late = 0.0;
    for i in 0..144000 {
        let x = if i < 12000 {
            (i as f32 * 0.12).sin() * 0.2
        } else {
            0.0
        };
        let y = freeze.process([x, -x], true);
        let e = y[0] * y[0] + y[1] * y[1];
        if (24000..48000).contains(&i) {
            early += e;
        }
        if i >= 120000 {
            late += e;
        }
    }
    assert!(
        late > early * 0.05 && late < early * 2.0 && early > 0.01,
        "{early} -> {late}"
    );
    for kind in [EffectKind::EchoOut, EffectKind::ReverbOut] {
        let mut effect = Effect::new(48000.0);
        effect.configure(EffectParams {
            kind,
            mix: 1.0,
            bypass: false,
            beats: 0.5,
            ..p
        });
        for _ in 0..15000 {
            effect.process([0.0; 2], true);
        }
        for _ in 0..48000 {
            assert_eq!(
                effect.process([0.4; 2], true),
                [0.0; 2],
                "{kind:?} admitted new input"
            );
        }
    }
}
#[test]
fn bandpass_passes_center_and_rejects_both_sides() {
    fn rms(hz: f32) -> f32 {
        let mut e = Effect::new(48000.0);
        e.configure(EffectParams {
            kind: EffectKind::BandPass,
            mix: 1.0,
            bypass: false,
            depth: 0.5,
            ..EffectParams::default()
        });
        let mut sum = 0.0;
        for i in 0..24000 {
            let x = (std::f32::consts::TAU * hz * i as f32 / 48000.0).sin() * 0.2;
            let y = e.process([x; 2], false);
            if i > 12000 {
                sum += y[0] * y[0];
            }
        }
        sum
    }
    let middle = rms(30.0 * 600.0f32.sqrt());
    assert!(middle > rms(60.0) * 20.0);
    assert!(middle > rms(10000.0) * 20.0);
}
#[test]
fn every_kind_extremes_bypass_send_and_type_switch_stay_bounded() {
    for sr in [44100.0, 48000.0, 96000.0] {
        let mut e = Effect::new(sr);
        for kind in EffectKind::ALL {
            for depth in [0.0, 1.0] {
                let p = EffectParams {
                    kind,
                    mix: 1.0,
                    bypass: false,
                    depth,
                    drive: depth,
                    feedback: 0.88,
                    size: depth,
                    damping: depth,
                    beats: 0.125,
                    ..EffectParams::default()
                };
                e.configure(p);
                for i in 0..sr as usize / 3 {
                    let x = (i as f32 * 0.18).sin() * 0.12;
                    let y = e.process([x, -x * 0.7], false);
                    assert!(
                        y.iter().all(|v| v.is_finite() && v.abs() < 5.0),
                        "{sr} {kind:?} {depth}: {y:?}"
                    );
                }
                e.configure(EffectParams { mix: 0.0, ..p });
                for _ in 0..(sr * 0.16) as usize {
                    e.process([0.1; 2], false);
                }
                assert_eq!(e.process([0.1; 2], false), [0.1; 2]);
                assert_eq!(e.process([0.1; 2], true), [0.0; 2]);
            }
        }
    }
}
#[test]
#[ignore = "release-only per-effect callback timing"]
fn catalogue_callback_budget() {
    let mut e = Effect::new(48000.0);
    let mut worst = (0.0, EffectKind::Echo);
    for kind in EffectKind::ALL {
        e.configure(EffectParams {
            kind,
            mix: 0.8,
            bypass: false,
            depth: 0.8,
            feedback: 0.88,
            ..EffectParams::default()
        });
        let mut timings = Vec::with_capacity(400);
        for block in 0..420 {
            let start = std::time::Instant::now();
            for i in 0..256 {
                let x = ((block * 256 + i) as f32 * 0.15).sin() * 0.2;
                std::hint::black_box(e.process([x, -x], false));
            }
            if block >= 20 {
                timings.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        timings.sort_by(f64::total_cmp);
        let p99 = timings[396];
        if p99 > worst.0 {
            worst = (p99, kind);
        }
        assert!(p99 < 0.12, "{kind:?} p99={p99:.4} ms");
    }
    eprintln!(
        "70 kinds, worst {:?} p99 {:.4} ms / 256 frames",
        worst.1, worst.0
    );
}

#[test]
fn reverb_and_delay_switches_fade_and_can_be_retriggered_or_opted_out() {
    for kind in [EffectKind::Reverb, EffectKind::Delay, EffectKind::Echo] {
        let mut effect = Effect::new(48_000.);
        let mut params = EffectParams {
            kind,
            mix: 0.7,
            bypass: false,
            ..Default::default()
        };
        effect.configure(params);
        let mut previous = [0.; 2];
        for _ in 0..48_000 {
            previous = effect.process([0.1; 2], true);
        }
        params.bypass = true;
        effect.configure(params);
        let first = effect.process([0.1; 2], true);
        assert!((first[0] - previous[0]).abs() < 0.001, "{kind:?}");
        for _ in 0..3600 {
            previous = effect.process([0.1; 2], true);
        }
        assert_ne!(
            previous, [0.; 2],
            "fade must still be audible at 75 ms: {kind:?}"
        );
        params.bypass = false;
        effect.configure(params);
        assert!((effect.process([0.1; 2], true)[0] - previous[0]).abs() < 0.001);
        for _ in 0..7200 {
            effect.process([0.1; 2], true);
        }
        params.bypass = true;
        effect.configure(params);
        for _ in 0..7200 {
            effect.process([0.1; 2], true);
        }
        assert_eq!(effect.process([0.1; 2], true), [0.; 2]);
        params.auto_fade = false;
        params.bypass = false;
        effect.configure(params);
        for _ in 0..480 {
            effect.process([0.1; 2], true);
        }
        params.bypass = true;
        effect.configure(params);
        for _ in 0..240 {
            effect.process([0.1; 2], true);
        }
        assert_eq!(effect.process([0.1; 2], true), [0.; 2]);
    }
}
