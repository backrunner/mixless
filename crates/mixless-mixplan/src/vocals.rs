//! Protect heard vocal phrases, independently of instrumental tonal foreground.
use mixless_protocol::{BarFeature, TrackAnalysis};

pub(crate) fn risk(bar: &BarFeature) -> f32 {
    bar.vocal_confidence.unwrap_or(bar.vocal_presence)
}

pub(crate) fn phrases(t: &TrackAnalysis) -> Vec<(f32, f32)> {
    let gap = 240. / t.tempo.global_bpm.max(30.);
    let mut result: Vec<(f32, f32)> = vec![];
    for bar in t.bars.iter().filter(|b| b.rms > 0.001 && risk(b) >= 0.45) {
        if let Some(last) = result.last_mut().filter(|last| {
            bar.start_sec - last.1 <= gap + 0.01
                || (bar.start_sec - last.1 <= 2. * gap + 0.01
                    && t.bars
                        .iter()
                        .filter(|b| {
                            b.start_sec >= last.1 - 0.01 && b.end_sec <= bar.start_sec + 0.01
                        })
                        .all(|b| risk(b) >= 0.25))
        }) {
            last.1 = bar.end_sec;
        } else {
            result.push((bar.start_sec, bar.end_sec));
        }
    }
    result
}

pub(crate) fn safe_exit(t: &TrackAnalysis, end: f32) -> bool {
    crate::continuity::voice_cut_safe(t, end)
}

pub(crate) fn safe_entry(t: &TrackAnalysis, start: f32) -> bool {
    if t.stems
        .as_ref()
        .is_some_and(|s| s.note_crossing(start, mixless_protocol::StemKind::Vocals))
        && crate::continuity::voice_active(t, start) > 0.2
    {
        return false;
    }
    // Starting a phrase is fine; a cue inside an already sung syllable is not.
    if crate::phrasing::boundary_quality(t, start) >= 0.6 {
        return true;
    }
    crate::continuity::voice_cut_safe(t, start)
}

pub(crate) fn last_end(t: &TrackAnalysis, start: f32, end: f32) -> Option<f32> {
    phrases(t)
        .into_iter()
        .filter(|&(a, b)| a < end - 0.05 && b > start)
        .map(|(_, b)| b)
        .max_by(f32::total_cmp)
}

/// Keep one singer in front, then use a measured breath or a long level/EQ
/// release. Completing an entire vocal section is not a hard constraint.
pub(crate) fn protect(plan: &mut mixless_protocol::MixPlan, t: &TrackAnalysis) -> Option<()> {
    if last_end(t, plan.t_in_a, plan.t_out_a).is_none() {
        return Some(());
    }
    let n = plan.summary.as_ref()?.length_bars as f32;
    if n < 4. {
        return None;
    }
    let g = crate::grid::Grid(t);
    let end = if t.moments.is_empty() {
        let end = last_end(t, plan.t_in_a, plan.t_out_a)?;
        if end
            > plan.t_out_a
                - (240. / t.tempo.global_bpm.max(30.)).min((plan.t_out_a - plan.t_in_a) * 0.25)
                + 0.05
        {
            return None;
        }
        end
    } else {
        crate::continuity::fade_start(t, plan.t_in_a, plan.t_out_a)
    };
    let hold = ((g.beat(end) - g.beat(plan.t_in_a)) / g.meter()).clamp(0., n - 1.);
    let ease = |v: f32| {
        let v = v.clamp(0., 1.);
        v * v * (3. - 2. * v)
    };
    let bass = plan.handoff_bar.unwrap_or(n * 0.55);
    let to_db = |gain: f32| (20. * gain.max(0.000016).log10()).clamp(-96., 6.);
    let lanes = &mut plan.lanes;
    for lane in [
        &mut lanes.xfader,
        &mut lanes.gain_a,
        &mut lanes.eq_a.mid,
        &mut lanes.eq_a.high,
        &mut lanes.filter_a.lp_hz,
        &mut lanes.filter_a.hp_hz,
        &mut lanes.fx_send_a,
        &mut lanes.eq_a.low,
        &mut lanes.eq_b.low,
        &mut lanes.eq_b.mid,
    ] {
        lane.nodes.clear();
    }
    for i in 0..=n as usize * 64 {
        let u = i as f32 / 64.;
        let p = if u <= hold {
            ease(u / hold.max(0.01)) / 3.
        } else {
            1. / 3. + 2. / 3. * ease((u - hold) / (n - hold))
        };
        let tail = ease((u - hold) / (n - hold));
        let cos = (p * std::f32::consts::FRAC_PI_2).cos().max(0.00002);
        let gain = if u <= hold {
            1. / cos
        } else {
            (1. - tail) / cos
        };
        lanes.xfader.nodes.push((u, 2. * p - 1.));
        lanes.gain_a.nodes.push((
            u,
            if u >= n {
                -96.
            } else {
                (20. * gain.max(0.00002).log10()).clamp(-96., 6.)
            },
        ));
        lanes.eq_a.mid.nodes.push((u, -12. * tail));
        lanes.eq_a.high.nodes.push((u, -3. * tail));
        lanes.filter_a.lp_hz.nodes.push((u, 20000.));
        lanes.filter_a.hp_hz.nodes.push((u, 20.));
        lanes.fx_send_a.nodes.push((u, 0.));
        lanes
            .eq_b
            .mid
            .nodes
            .push((u, -18. * (1. - ease((tail - 0.6) / 0.4))));
        // Recompute bass compensation against the new vocal-preserving levels.
        // Reusing the old EQ curve would boost both bass lines during the hold.
        let x = p * std::f32::consts::FRAC_PI_2;
        let exchange = ease((u - bass + 0.125) / 0.25) * std::f32::consts::FRAC_PI_2;
        lanes
            .eq_a
            .low
            .nodes
            .push((u, to_db(exchange.cos().max(0.) / (cos * gain).max(0.00002))));
        let level_b = x.sin() * 10f32.powf(lanes.gain_b.sample(u) / 20.);
        lanes
            .eq_b
            .low
            .nodes
            .push((u, to_db(exchange.sin().max(0.) / level_b.max(0.00002))));
    }
    plan.stages = vec![
        mixless_protocol::MixStage {
            start_bar: 0.,
            end_bar: hold,
            label: "Layer rhythm · hold vocal foreground".into(),
        },
        mixless_protocol::MixStage {
            start_bar: hold,
            end_bar: n,
            label: "Vocal gap / soft vocal exit".into(),
        },
    ];
    plan.stages.retain(|s| s.end_bar > s.start_bar);
    Some(())
}
