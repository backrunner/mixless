//! A deliberately filtered rhythmic introduction when a full harmonic blend is
//! unsupported. This is band-limited mixing, never a claim of stem separation.
use mixless_protocol::{MixPlan, MixStage, StrategyId, TrackAnalysis};

pub(crate) fn eligible(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let bars: Vec<_> = t
        .bars
        .iter()
        .filter(|b| b.start_sec < end && b.end_sec > start)
        .collect();
    bars.len() >= 4
        && bars.iter().all(|b| {
            b.rms > 0.001
                && b.vocal_confidence.is_some_and(|v| v < 0.25)
                && b.onset_density >= 1.
                && b.high_db > b.mid_db - 24.
        })
        && bars.iter().map(|b| b.kick_salience).sum::<f32>() / bars.len() as f32 >= 0.55
}

/// Dense mixture evidence permits a quieter, band-limited layer even when the
/// whole-track key or voice classifier is uncertain. Never infer this from a
/// missing analysis, and never describe these bands as isolated stems.
pub(crate) fn layerable(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    let first = t.moments.partition_point(|m| m.end_sec <= start);
    let last = t.moments.partition_point(|m| m.start_sec < end);
    let frames = &t.moments[first..last];
    !frames.is_empty()
        && frames.len() as f32 * 0.05 >= (end - start) * 0.9
        && frames
            .iter()
            .filter(|m| m.rms > 0.001 && m.band_db[2] > m.band_db[1] - 30.)
            .count() as f32
            / frames.len() as f32
            > 0.8
}

pub(crate) fn arrange(p: &mut MixPlan, a: &TrackAnalysis, b: &TrackAnalysis) -> Option<()> {
    let n = p.summary.as_ref()?.length_bars as f32;
    let g = crate::grid::Grid(a);
    let fade = crate::continuity::fade_start(a, p.t_in_a, p.t_out_a);
    let peak_end = crate::drops::peaks(a)
        .into_iter()
        .filter(|(start, end)| *start < p.t_out_a - 0.05 && *end > p.t_in_a)
        .map(|(_, end)| end)
        .fold(p.t_in_a, f32::max);
    let hold = ((g.beat(fade.max(peak_end)) - g.beat(p.t_in_a)) / g.meter()).max(0.);
    if n < 4. || n - hold < 0.95 {
        return None;
    }
    let match_bands = crate::spectrum::compare(a, b, &p.outgoing_source, &p.incoming_source, n);
    let ease = |t: f32| {
        let t = t.clamp(0., 1.);
        (t * t * t * (10. + t * (-15. + 6. * t))).clamp(0., 1.)
    };
    let db = |g: f32| (20. * g.max(0.000016).log10()).clamp(-96., 6.);
    let lanes = &mut p.lanes;
    for l in [
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
        &mut lanes.filter_a.hp_hz,
        &mut lanes.filter_b.hp_hz,
        &mut lanes.filter_b.lp_hz,
        &mut lanes.fx_send_a,
        &mut lanes.fx_send_b,
    ] {
        l.nodes.clear();
    }
    for i in 0..=n as usize * 64 {
        let u = i as f32 / 64.;
        let closing = ease((u - hold) / (n - hold));
        let angle = (ease(u / hold.max(1.)) / 3. + 2. / 3. * closing) * std::f32::consts::FRAC_PI_2;
        let a_level = 1. - closing;
        let reveal = ease((closing - 0.65) / 0.35);
        lanes
            .xfader
            .nodes
            .push((u, (angle / std::f32::consts::FRAC_PI_4 - 1.).clamp(-1., 1.)));
        lanes.gain_a.nodes.push((
            u,
            if u == n {
                -96.
            } else {
                db(a_level / angle.cos().max(0.00002))
            },
        ));
        lanes.gain_b.nodes.push((u, 0.));
        lanes.eq_a.low.nodes.push((u, -12. * closing));
        lanes.eq_b.low.nodes.push((u, db(reveal)));
        lanes.eq_a.mid.nodes.push((u, 0.));
        lanes
            .eq_b
            .mid
            .nodes
            .push((u, -match_bands.mid_cut_db * (1. - reveal)));
        lanes.eq_a.high.nodes.push((u, 0.));
        lanes
            .eq_b
            .high
            .nodes
            .push((u, -match_bands.high_cut_db * (1. - reveal)));
        lanes.filter_a.lp_hz.nodes.push((u, 20000.));
        lanes.filter_a.hp_hz.nodes.push((u, 20.));
        lanes.filter_b.hp_hz.nodes.push((
            u,
            (match_bands.highpass_hz.ln() + reveal * (20f32 / match_bands.highpass_hz).ln())
                .exp()
                .clamp(20., 2500.),
        ));
        lanes.filter_b.lp_hz.nodes.push((u, 20000.));
        lanes.fx_send_a.nodes.push((u, 0.));
        lanes.fx_send_b.nodes.push((u, 0.));
    }
    p.summary.as_mut()?.strategy = StrategyId::FilterSweep;
    p.summary.as_mut()?.used_fallback = true;
    p.handoff_bar = Some(hold + (n - hold) * 0.8);
    p.stages = vec![
        MixStage {
            start_bar: 0.,
            end_bar: (hold * 0.5).min(2.),
            label: "Introduce complementary frequency band".into(),
        },
        MixStage {
            start_bar: (hold * 0.5).min(2.),
            end_bar: hold,
            label: "Hold outgoing foreground".into(),
        },
        MixStage {
            start_bar: hold,
            end_bar: n,
            label: "Close outgoing · reveal incoming bands".into(),
        },
    ];
    p.stages.retain(|s| s.end_bar > s.start_bar);
    Some(())
}
