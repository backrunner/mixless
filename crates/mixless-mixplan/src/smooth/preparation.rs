//! Source-clock preparation before the incoming deck becomes audible.
use super::*;

pub(super) fn candidate(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    out: f32,
    input: f32,
    mode: TransitionMode,
    length: u16,
    minimum_score: f32,
) -> Option<MixPlan> {
    if mode != TransitionMode::BeatBlend || options.strategy.is_some() || length < 4 {
        return None;
    }
    let ga = Grid(ctx.outgoing);
    let gb = Grid(ctx.incoming);
    let join = out - length as f32 * ga.meter();
    let a_bpm = ga.bpm(join) * ctx.offset_a.rate;
    let b_bpm = gb.bpm(input) * ctx.offset_b.rate;
    let ratio = a_bpm / b_bpm;
    let factor = if (ratio / 2. - 1.).abs() < 0.08 {
        0.5
    } else if (ratio / 0.5 - 1.).abs() < 0.08 {
        2.
    } else {
        1.
    };
    if (ratio * factor - 1.).abs() <= 0.08 {
        return None;
    }
    let target = (a_bpm * b_bpm / factor).sqrt();
    let rate = target / ga.bpm(join);
    // Share the correction; neither deck may exceed the existing 8% budget.
    // Absolute limits also prevent accumulated offsets drifting indefinitely.
    if !(0.92..=1.08).contains(&rate)
        || !(0.92..=1.08).contains(&(target * factor / gb.bpm(input)))
        || !(0.92..=1.08).contains(&(rate / ctx.offset_a.rate))
    {
        return None;
    }
    let change = (rate / ctx.offset_a.rate).ln().abs();
    // Quintic easing has maximum derivative 1.875; integrate and check again.
    let seconds = 1.875 * change / MAX_LOG_TEMPO_PER_SEC * 1.1;
    let needed = (seconds * a_bpm.max(target) / (60. * ga.meter()))
        .ceil()
        .max(1.);
    // Start at an evidenced phrase where possible, rather than an arbitrary bar.
    let begin = if ctx.outgoing.phrase_boundaries.is_empty() {
        join - needed * ga.meter()
    } else {
        ctx.outgoing
            .phrase_boundaries
            .iter()
            .filter(|p| p.confidence >= 0.5)
            .map(|p| ga.beat(p.time_sec))
            .filter(|beat| *beat <= join - needed * ga.meter())
            .max_by(f32::total_cmp)?
    };
    let bars = (join - begin) / ga.meter();
    let start = ga.sec(begin);
    if begin < 0.
        || (bars - bars.round()).abs() > 0.01
        || bars + length as f32 > 128.
        || start
            < options
                .earliest_outgoing_sec
                .max(options.outgoing_entry_sec)
        || !grid_reliable(ctx.outgoing, start, ga.sec(join))
    {
        return None;
    }
    let prepared = PlanContext {
        outgoing: ctx.outgoing,
        incoming: ctx.incoming,
        cues_out: ctx.cues_out,
        cues_in: ctx.cues_in,
        offset_a: mixless_protocol::PerformanceOffset {
            rate,
            ..ctx.offset_a
        },
        offset_b: ctx.offset_b,
    };
    let held = PlannerOptions {
        strategy: Some(S::EnergyHold),
        ..options.clone()
    };
    let mut plan =
        super::candidate::direct(&prepared, &held, out, input, mode, length, minimum_score)?;
    // Preparation earns no duration reward. The original overlap was scored.
    plan.summary.as_mut()?.score -= change * 0.1;
    if plan.summary.as_ref()?.score <= minimum_score {
        return None;
    }
    prepend(ctx, &mut plan, begin, bars.round(), rate)?;
    Some(plan)
}

fn shift(line: &mut Polyline, bars: f32, hold: bool) {
    let initial = line.sample(0.);
    for (u, _) in &mut line.nodes {
        *u += bars;
    }
    if hold && !line.nodes.is_empty() {
        line.nodes.insert(0, (0., initial));
    }
}

fn prepend(
    ctx: &PlanContext<'_>,
    p: &mut MixPlan,
    begin: f32,
    bars: f32,
    target: f32,
) -> Option<()> {
    let ga = Grid(ctx.outgoing);
    let mut clock = vec![(0., 0.)];
    let mut source = vec![(0., ga.sec(begin))];
    let mut rates = vec![(0., ctx.offset_a.rate)];
    let mut master = vec![(0., ga.bpm(begin) * ctx.offset_a.rate)];
    let mut wall = 0.;
    let mut prev_sec = ga.sec(begin);
    let mut prev_rate = ctx.offset_a.rate;
    let mut prev_bpm = ga.bpm(begin) * prev_rate;
    for j in 1..=bars as usize * 64 {
        let u = j as f32 / 64.;
        let beat = begin + u * ga.meter();
        let sec = ga.sec(beat);
        let rate = ctx.offset_a.rate * ((target / ctx.offset_a.rate).ln() * ease(u / bars)).exp();
        let dt = 2. * (sec - prev_sec) / (rate + prev_rate);
        let bpm = ga.bpm(beat) * rate;
        if dt <= 0. || (bpm / prev_bpm).ln().abs() / dt > MAX_LOG_TEMPO_PER_SEC * 1.02 {
            return None;
        }
        wall += dt;
        clock.push((u, wall));
        source.push((u, sec));
        rates.push((u, rate));
        master.push((u, bpm));
        prev_sec = sec;
        prev_rate = rate;
        prev_bpm = bpm;
    }
    let l = &mut p.lanes;
    for line in [
        &mut l.xfader,
        &mut l.gain_a,
        &mut l.gain_b,
        &mut l.eq_a.low,
        &mut l.eq_a.mid,
        &mut l.eq_a.high,
        &mut l.eq_b.low,
        &mut l.eq_b.mid,
        &mut l.eq_b.high,
        &mut l.filter_a.hp_hz,
        &mut l.filter_a.lp_hz,
        &mut l.filter_b.hp_hz,
        &mut l.filter_b.lp_hz,
        &mut l.fx_send_a,
        &mut l.fx_send_b,
        &mut l.rate_b,
        &mut l.pitch_a,
        &mut l.pitch_b,
    ] {
        shift(line, bars, true);
    }
    for op in [&mut l.loop_a, &mut l.loop_b].into_iter().flatten() {
        op.on_bar += bars;
        op.off_bar += bars;
    }
    for op in [&mut l.scratch_a, &mut l.scratch_b].into_iter().flatten() {
        op.on_bar += bars;
        op.peak_bar += bars;
        op.off_bar += bars;
    }
    for (line, mut prefix, delta) in [
        (&mut p.clock, clock, wall),
        (&mut p.outgoing_source, source, 0.),
        (&mut l.rate_a, rates, 0.),
        (&mut p.master_bpm, master, 0.),
    ] {
        prefix.pop();
        prefix.extend(line.nodes.iter().map(|(u, v)| (u + bars, v + delta)));
        line.nodes = prefix;
    }
    shift(&mut p.incoming_source, bars, false);
    if let Some(stems) = &mut p.stem_mix {
        for line in stems.outgoing.iter_mut().chain(&mut stems.incoming) {
            shift(line, bars, true);
        }
    }
    for stage in &mut p.stages {
        stage.start_bar += bars;
        stage.end_bar += bars;
    }
    p.stages.insert(
        0,
        mixless_protocol::MixStage {
            start_bar: 0.,
            end_bar: bars,
            label: "Prepare shared tempo · incoming silent".into(),
        },
    );
    p.incoming_start_bar += bars;
    p.handoff_bar = p.handoff_bar.map(|u| u + bars);
    p.summary.as_mut()?.length_bars += bars as u16;
    p.t_in_a = ga.sec(begin);
    p.outgoing_offset = ctx.offset_a;
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::SectionLabel as L;

    #[test]
    fn preparation_preserves_entry_and_shares_stretch_without_consuming_incoming() {
        for scale in [0.8, 1., 1.2] {
            let mut a = crate::tests::track(1, 135. * scale, "8A", L::Outro, 96, 0.9, 0.2);
            // The tested internal exit has a measured arrangement edge;
            // tempo preparation alone cannot invent a phrase ending.
            let edge = 64. * 240. / (135. * scale);
            a.sections[0].end_sec = edge;
            a.sections.push(mixless_protocol::Section {
                start_sec: edge,
                end_sec: a.duration_sec,
                label: L::Outro,
            });
            let b = crate::tests::track(2, 150. * scale, "8A", L::Intro, 96, 0.9, 0.2);
            let ctx = PlanContext {
                outgoing: &a,
                incoming: &b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            };
            let p = candidate(
                &ctx,
                &PlannerOptions::default(),
                256.,
                0.,
                TransitionMode::BeatBlend,
                16,
                -1.,
            )
            .unwrap();
            let launch = p.incoming_start_bar;
            assert!(launch > 0.);
            assert_eq!(p.lanes.rate_a.sample(0.), 1.);
            assert!((p.lanes.rate_a.sample(launch) - (150f32 / 135.).sqrt()).abs() < 0.001);
            assert!((p.incoming_offset_end.rate - (135f32 / 150.).sqrt()).abs() < 0.001);
            assert_eq!(p.incoming_source.nodes[0].0, launch);
            assert!((p.incoming_source.sample(launch) - p.t_in_b).abs() < 0.001);
            assert!(p.clock.nodes.windows(2).all(|w| w[1].1 > w[0].1));
            let late = PlannerOptions {
                earliest_outgoing_sec: p.t_in_a + 0.1,
                ..Default::default()
            };
            assert!(candidate(&ctx, &late, 256., 0., TransitionMode::BeatBlend, 16, -1.).is_none());
        }
    }

    #[test]
    fn weak_grid_and_extreme_tempo_cannot_borrow_preparation_permission() {
        let mut a = crate::tests::track(1, 135., "8A", L::Outro, 96, 0.9, 0.2);
        let b = crate::tests::track(2, 175., "8A", L::Intro, 96, 0.9, 0.2);
        let run = |a: &TrackAnalysis, b: &TrackAnalysis| {
            let ctx = PlanContext {
                outgoing: a,
                incoming: b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            };
            candidate(
                &ctx,
                &PlannerOptions::default(),
                256.,
                0.,
                TransitionMode::BeatBlend,
                16,
                -1.,
            )
        };
        assert!(run(&a, &b).is_none());
        let b = crate::tests::track(2, 150., "8A", L::Intro, 96, 0.9, 0.2);
        a.tempo.beats.clear();
        assert!(run(&a, &b).is_none());
    }
}
