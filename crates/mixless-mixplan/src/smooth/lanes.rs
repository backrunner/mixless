//! Level, EQ and filter envelopes, independent of candidate ranking.
use super::*;
pub(super) fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    (t * t * t * (10. + t * (-15. + 6. * t))).clamp(0., 1.)
}
pub(super) fn line(nodes: &[(f32, f32)]) -> Polyline {
    Polyline {
        nodes: nodes.to_vec(),
    }
}
pub(super) fn neutral() -> AutomationLanes {
    let zero = Polyline::constant(0.);
    let eq = EqLane {
        low: zero.clone(),
        mid: zero.clone(),
        high: zero.clone(),
    };
    let filter = FilterLane {
        lp_hz: Polyline::constant(20000.),
        hp_hz: Polyline::constant(20.),
    };
    AutomationLanes {
        xfader: zero.clone(),
        gain_a: zero.clone(),
        gain_b: zero.clone(),
        eq_a: eq.clone(),
        eq_b: eq,
        filter_a: filter.clone(),
        filter_b: filter,
        fx_send_a: zero.clone(),
        fx_send_b: zero.clone(),
        rate_a: zero.clone(),
        rate_b: zero.clone(),
        pitch_a: zero.clone(),
        pitch_b: zero,
        loop_a: None,
        loop_b: None,
        scratch_a: None,
        scratch_b: None,
    }
}

pub(super) fn shape(
    lanes: &mut AutomationLanes,
    blend: bool,
    loop_roll: bool,
    n: f32,
    handoff: f32,
    start_b: f32,
    rhythmic_handoff: bool,
    meter: f32,
    trim_b: f32,
    vocal_a: f32,
    vocal_b: f32,
    outgoing_bpm: f32,
    technique: Technique,
) {
    if blend {
        let middle = handoff;
        let width = if rhythmic_handoff {
            0.5 / meter
        } else {
            (n * 0.125).min(2.)
        };
        // One bass foreground until the phrase downbeat. The old -96 dB ramp
        // spread across 8 bars created a long bass hole; exchange within one beat.
        // Smooth both sides independently, so the actual exchange is centered
        // at unity-power even when its phrase is away from n/2.
        lanes.xfader.nodes = (0..=n as usize * 64)
            .map(|j| {
                let u = j as f32 / 64.;
                let value = if u <= middle {
                    -1. + ease(u / middle)
                } else {
                    ease((u - middle) / (n - middle))
                };
                (u, value)
            })
            .collect();
        lanes.gain_a = line(&[(0., 0.), (n - 0.001, 0.), (n, KILL)]);
        lanes.gain_b = Polyline::constant(trim_b);
        lanes.eq_a.low.nodes.clear();
        lanes.eq_b.low.nodes.clear();
        lanes.eq_a.high.nodes.clear();
        lanes.eq_b.high.nodes.clear();
        for j in 0..=n as usize * 64 {
            let u = j as f32 / 64.;
            // Introduce the incoming hats gently before exchanging the bass.
            // A's initial tone and B's final tone stay neutral.
            lanes
                .eq_b
                .high
                .nodes
                .push((u, -3. * (1. - ease(u / middle))));
            lanes
                .eq_a
                .high
                .nodes
                .push((u, -3. * ease((u - middle * 0.5) / (n - middle * 0.5))));
            let x = (lanes.xfader.sample(u) + 1.) * std::f32::consts::FRAC_PI_4;
            let handoff = ease((u - (middle - width)) / (2. * width)) * std::f32::consts::FRAC_PI_2;
            let low_a = handoff.cos().max(0.);
            let low_b = handoff.sin().max(0.);
            let to_db = |gain: f32| {
                if gain < 0.00002 {
                    KILL
                } else {
                    20. * gain.log10()
                }
            };
            lanes
                .eq_a
                .low
                .nodes
                .push((u, to_db(low_a / x.cos().max(0.001)).clamp(KILL, 6.)));
            lanes
                .eq_b
                .low
                .nodes
                .push((u, to_db(low_b / x.sin().max(0.001)).clamp(KILL, 6.)));
        }
        // Do not overlap two midrange foregrounds. Mild ducking only on the
        // secondary instrumental layer, never repeated modulation of a vocal.
        if vocal_a > 0.5 {
            lanes.eq_b.mid = line(&[(0., -6.), (middle, -6.), (n, 0.)]);
        }
        if vocal_b > 0.5 {
            lanes.eq_a.mid = line(&[(0., 0.), (middle, -6.), (n, -12.)]);
        }
    } else if !loop_roll {
        let handoff = start_b;
        // Finish the dry fade just before the downbeat, then open B fully on
        // its first kick. FX is inserted only when the policy says the gap or
        // tonal conflict needs it; clean rhythmic cuts stay dry.
        let fade = (0.04 * outgoing_bpm / (60. * meter)).min(0.125);
        lanes.xfader = line(&[(0., -1.), (handoff - fade, -1.), (handoff, 1.)]);
        lanes.gain_a = line(&[(0., 0.), (handoff - fade, 0.), (handoff, KILL)]);
        lanes.gain_b = Polyline::constant(trim_b);
        lanes.eq_a.low = line(&[
            (0., 0.),
            ((handoff - 0.25).max(0.), 0.),
            (handoff - fade, -12.),
            (handoff, KILL),
        ]);
        // Avoid duplicate knots for exceptionally short bridges.
        lanes.eq_a.low.nodes.dedup_by(|a, b| a.0 == b.0);
        if matches!(technique, Technique::EchoOut | Technique::FilterBridge) {
            lanes.filter_a.lp_hz = line(&[
                (0., 20000.),
                ((handoff - 0.5).max(0.), 20000.),
                (handoff - 0.25, 2000.),
                (handoff, 1200.),
            ]);
            lanes.filter_a.lp_hz.nodes.dedup_by(|a, b| a.0 == b.0);
            lanes.fx_send_a = if technique == Technique::EchoOut {
                line(&[
                    (0., 0.),
                    ((handoff - 0.5).max(0.), 0.),
                    (handoff - 0.25, 0.18),
                    (handoff, 0.),
                ])
            } else {
                Polyline::constant(0.)
            };
            lanes.fx_send_a.nodes.dedup_by(|a, b| a.0 == b.0);
        }
        if matches!(
            technique,
            Technique::DryCut | Technique::DropCut | Technique::ScratchCut
        ) {
            // A completed build can hand directly to the new drop. Keep its
            // buildup intact and avoid an FX tail masking the incoming first kick.
            lanes.filter_a = neutral().filter_a;
            lanes.fx_send_a = Polyline::constant(0.);
            lanes.eq_a.low = line(&[(0., 0.), (handoff - fade, 0.), (handoff, KILL)]);
        }
    }
}
