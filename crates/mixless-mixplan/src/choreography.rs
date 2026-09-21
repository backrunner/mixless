//! Compose independent band exchanges, level rides, filter and optional FX.
//! All nonlinear curves are sampled on the host; the callback only reads them.
use crate::policy::Technique;
use mixless_protocol::{AutomationLanes, MixStage, Polyline, SectionLabel, TransitionMode};
const KILL: f32 = -96.;
fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    (t * t * t * (10. + t * (-15. + 6. * t))).clamp(0., 1.)
}
fn db(x: f32) -> f32 {
    if x < 0.00002 {
        KILL
    } else {
        (20. * x.log10()).clamp(KILL, 6.)
    }
}
fn stage(start: f32, end: f32, label: &str) -> MixStage {
    MixStage {
        start_bar: start,
        end_bar: end,
        label: label.into(),
    }
}

fn soften(line: &mut Polyline, log: bool) {
    if line.nodes.len() < 2 {
        return;
    }
    let mut nodes = Vec::new();
    for w in line.nodes.windows(2) {
        let ((a, x), (b, y)) = (w[0], w[1]);
        let count = ((b - a) * 64.).ceil().clamp(8., 256.) as usize;
        for i in 0..count {
            let t = i as f32 / count as f32;
            let v = ease(t);
            nodes.push((
                a + (b - a) * t,
                if log {
                    (x.ln() + (y.ln() - x.ln()) * v)
                        .exp()
                        .clamp(x.min(y), x.max(y))
                } else {
                    x + (y - x) * v
                },
            ));
        }
    }
    nodes.push(*line.nodes.last().unwrap());
    line.nodes = nodes;
}

pub(super) fn arrange(
    lanes: &mut AutomationLanes,
    mode: TransitionMode,
    n: f32,
    bass: f32,
    meter: f32,
    trim: f32,
    harmonic: bool,
    rhythmic: bool,
    voice_a: f32,
    voice_b: f32,
    _section: SectionLabel,
    technique: Technique,
) -> Vec<MixStage> {
    if mode != TransitionMode::BeatBlend {
        for line in [
            &mut lanes.xfader,
            &mut lanes.gain_a,
            &mut lanes.gain_b,
            &mut lanes.eq_a.low,
            &mut lanes.fx_send_a,
        ] {
            soften(line, false);
        }
        soften(&mut lanes.filter_a.lp_hz, true);
        let label = match technique {
            Technique::EchoOut => "Echo tail",
            Technique::FilterBridge => "Filter exit",
            Technique::LoopRoll => "Loop layer",
            Technique::Spinback => "Spinback",
            _ => "Phrase cut",
        };
        if let Some(op) = &lanes.loop_b {
            return vec![
                stage(0., op.off_bar, "Introduce drum loop"),
                stage(
                    op.off_bar,
                    op.off_bar + 0.25,
                    "Bass exchange · release loop",
                ),
                stage(op.off_bar + 0.25, n, "Blend out outgoing"),
            ];
        }
        return vec![
            stage(0., (n - 0.5).max(0.01), "Hold phrase"),
            stage((n - 0.5).max(0.01), n + 0.25, label),
        ];
    }
    if n < 24. || !harmonic || !rhythmic || voice_a.max(voice_b) >= 0.45 {
        // Preserve the shorter beat blend and its bass-power compensation.
        soften(&mut lanes.eq_a.mid, false);
        soften(&mut lanes.eq_b.mid, false);
        return vec![
            stage(0., bass, "Introduce rhythm"),
            stage(bass, (bass + 0.25).min(n), "Bass exchange"),
            stage((bass + 0.25).min(n), n, "Release outgoing"),
        ];
    }
    let intro = (n * 0.12).min(4.).min(bass * 0.4);
    let exit = (n * 0.12).min(8.);
    let high_start = intro;
    let high_end = (bass * 0.85).max(high_start + 0.5);
    let mid_start = (bass + 0.5).max(n - exit * 2.);
    let mid_end = n - exit * 0.15;
    // A compatible layered mix needs no automatic sweep or echo. EQ already
    // makes room; reserve effects for the explicit bridge policy.
    for line in [
        &mut lanes.xfader,
        &mut lanes.gain_a,
        &mut lanes.gain_b,
        &mut lanes.eq_a.low,
        &mut lanes.eq_b.low,
        &mut lanes.eq_a.mid,
        &mut lanes.eq_b.mid,
        &mut lanes.eq_a.high,
        &mut lanes.eq_b.high,
        &mut lanes.filter_a.lp_hz,
        &mut lanes.fx_send_a,
    ] {
        line.nodes.clear();
    }
    for j in 0..=n as usize * 64 {
        let u = j as f32 / 64.;
        // Open the mixer, hold the layered middle, then finish the crossfade.
        let xf = if u < intro {
            -1. + ease(u / intro)
        } else if u > n - exit {
            ease((u - (n - exit)) / exit)
        } else {
            0.
        };
        let level_a = 1. - ease((u - mid_end) / (n - mid_end));
        let level_b = ease(u / intro);
        lanes.xfader.nodes.push((u, xf));
        lanes.gain_a.nodes.push((u, db(level_a)));
        lanes.gain_b.nodes.push((u, (db(level_b) + trim).max(KILL)));
        let x = (xf + 1.) * std::f32::consts::FRAC_PI_4;
        let envelope_a = x.cos() * level_a;
        let envelope_b = x.sin() * level_b;
        let bass_width = 0.5 / meter;
        let phases = [
            ease((u - (bass - bass_width)) / (2. * bass_width)),
            ease((u - mid_start) / (mid_end - mid_start)),
            ease((u - high_start) / (high_end - high_start)),
        ];
        for (band, p) in phases.into_iter().enumerate() {
            let angle = p * std::f32::consts::FRAC_PI_2;
            let av = db(angle.cos().max(0.) / envelope_a.max(0.00001));
            let bv = db(angle.sin().max(0.) / envelope_b.max(0.00001));
            let (a, b) = match band {
                0 => (&mut lanes.eq_a.low, &mut lanes.eq_b.low),
                1 => (&mut lanes.eq_a.mid, &mut lanes.eq_b.mid),
                _ => (&mut lanes.eq_a.high, &mut lanes.eq_b.high),
            };
            a.nodes.push((u, av));
            b.nodes.push((u, bv));
        }
        lanes.filter_a.lp_hz.nodes.push((u, 20000.));
        lanes.fx_send_a.nodes.push((u, 0.));
    }
    vec![
        stage(0., intro, "Introduce layer"),
        stage(intro, high_end, "Exchange highs"),
        stage(high_end, bass, "Hold layered mix"),
        stage(bass, bass + 0.5, "Exchange bass"),
        stage(bass + 0.5, mid_start, "Hold melody"),
        stage(mid_start, n - exit * 0.15, "Exchange mids"),
        stage(mid_end, n, "Close outgoing level"),
    ]
    .into_iter()
    .filter(|s| s.end_bar > s.start_bar)
    .collect()
}
