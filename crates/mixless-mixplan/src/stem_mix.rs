//! Optional source envelopes. Existing whole-track EQ remains the safe fallback
//! when the host has not yet loaded both aligned stem buffers.
use super::*;
use mixless_protocol::{Polyline, StemKind, StemMix};

pub(super) fn arrange(ctx: &PlanContext<'_>, plan: &mut MixPlan, stem_playback: bool) {
    // A stem-layered candidate already carries deliberate envelopes.
    if plan.stem_mix.is_some() {
        return;
    }
    let Some(summary) = &plan.summary else {
        return;
    };
    let n = summary.length_bars as f32;
    let duration = plan.clock.sample(n);
    let (Some(a), Some(b)) = (&ctx.outgoing.stems, &ctx.incoming.stems) else {
        return;
    };
    if n <= 0. || duration <= 0.3 {
        return;
    }
    let source = |out: bool, u: f32| {
        let (curve, start, op) = if out {
            (&plan.outgoing_source, plan.t_in_a, &plan.lanes.loop_a)
        } else {
            (&plan.incoming_source, plan.t_in_b, &plan.lanes.loop_b)
        };
        let sec = if curve.nodes.is_empty() {
            start + plan.clock.sample(u)
        } else {
            curve.sample(u)
        };
        if let Some(op) = op.as_ref().filter(|op| u >= op.on_bar && u < op.off_bar) {
            let sr = if out {
                ctx.outgoing.sample_rate
            } else {
                ctx.incoming.sample_rate
            } as f32;
            let start = op.start_src_frame as f32 / sr;
            let length = op.length_src_frames as f32 / sr;
            start + (sec - start).rem_euclid(length.max(0.001))
        } else {
            sec
        }
    };
    // Follow the actual gain crossover, not a fixed number of bars after a drop.
    let center = (0..=128)
        .map(|i| n * i as f32 / 128.)
        .min_by(|x, y| {
            let balance =
                |u: f32| (plan.lanes.gain_a.sample(u) - plan.lanes.gain_b.sample(u)).abs();
            balance(*x).total_cmp(&balance(*y))
        })
        .unwrap_or(n * 0.5);
    let mut vocals = 0;
    let mut drums = 0;
    for i in 0..128 {
        let u = n * (i as f32 + 0.5) / 128.;
        if u < plan.incoming_start_bar
            || plan.lanes.gain_a.sample(u) < -24.
            || plan.lanes.gain_b.sample(u) < -24.
        {
            continue;
        }
        let (Some(a), Some(b)) = (a.frame_at(source(true, u)), b.frame_at(source(false, u))) else {
            continue;
        };
        vocals += usize::from(
            a.vocal_activity > 0.35 && b.vocal_activity > 0.35 && a.rms[0].min(b.rms[0]) > 0.0015,
        );
        drums += usize::from(
            a.rms[1].min(b.rms[1]) > 0.005 && a.band_db[1][0].min(b.band_db[1][0]) > -42.,
        );
    }
    let clash = if !plan.outgoing_source.nodes.is_empty() && !plan.incoming_source.nodes.is_empty()
    {
        crate::progression::tension(
            ctx.outgoing,
            ctx.incoming,
            &plan.outgoing_source,
            &plan.incoming_source,
            n,
            plan.lanes.pitch_a.sample(center),
            plan.lanes.pitch_b.sample(center),
        )
    } else {
        0.
    };
    let enabled = [
        vocals > 2,
        drums > 8,
        clash > 0.55,
        drums > 8 || clash > 0.55,
    ];
    if !enabled.iter().any(|v| *v) {
        return;
    }
    let voice_center = (0..=128)
        .map(|i| n * i as f32 / 128.)
        .filter(|u| *u > plan.incoming_start_bar && *u < n && (*u - center).abs() < n * 0.3)
        .filter(|u| crate::continuity::voice_gap(ctx.outgoing, source(true, *u)) == Some(true))
        .min_by(|x, y| (x - center).abs().total_cmp(&(y - center).abs()))
        .unwrap_or(center);
    let last = plan.clock.nodes.last().map_or(n, |p| p.0);
    let mut result = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    for i in 0..=128 {
        let u = n * i as f32 / 128.;
        let time = plan.clock.sample(u);
        for stem in 0..4 {
            let center_sec = plan
                .clock
                .sample(if stem == 0 { voice_center } else { center });
            // Envelope duration scales with the chosen overlap and is bounded
            // in seconds for smooth controls, not a musical entry constraint.
            let width = if stem == StemKind::Bass.index() {
                (plan.clock.sample((center + 0.125).min(n))
                    - plan.clock.sample((center - 0.125).max(0.)))
                .max(0.01)
            } else {
                (duration * 0.25).clamp(0.15, 8.).min(duration)
            };
            let x = ((time - center_sec) / width + 0.5).clamp(0., 1.);
            let x = x * x * (3. - 2. * x);
            let floor = [0.18, 0.35, 0.25, 0.][stem];
            let (ga, gb) = if enabled[stem] {
                (1. - (1. - floor) * x, floor + (1. - floor) * x)
            } else {
                (1., 1.)
            };
            result.outgoing[stem].nodes.push((
                u,
                if i == 0 || u <= plan.incoming_start_bar {
                    1.
                } else {
                    ga
                },
            ));
            result.incoming[stem]
                .nodes
                .push((u, if i == 128 { 1. } else { gb }));
        }
    }
    if last > n {
        for line in result.outgoing.iter_mut().chain(&mut result.incoming) {
            line.nodes.push((last, line.nodes.last().unwrap().1));
        }
    }
    // This relies on stem playback, which the plan then requires.
    if stem_playback && enabled[0] && release_mid(plan, &result.outgoing[StemKind::Vocals.index()])
    {
        plan.requires_stems = true;
    }
    plan.stem_mix = Some(result);
}

/// Whole-track vocal protection keeps the incoming mids down until the
/// outgoing voice has faded. Once the outgoing vocal stem is out that cut only
/// leaves an energy hole, so release it as the stem leaves. Returns whether
/// there was a cut to release; the caller must require stem playback.
fn release_mid(plan: &mut MixPlan, outgoing_vocal: &Polyline) -> bool {
    let mid = &mut plan.lanes.eq_b.mid;
    if !mid.nodes.iter().any(|(_, db)| *db < -0.5) {
        return false;
    }
    let floor = 0.18;
    // Sample on both grids so the release follows the stem, not the spacing
    // of the original EQ nodes.
    let mut points: Vec<f32> = mid
        .nodes
        .iter()
        .chain(&outgoing_vocal.nodes)
        .map(|(u, _)| *u)
        .collect();
    points.sort_by(f32::total_cmp);
    points.dedup_by(|a, b| (*a - *b).abs() < 0.0001);
    mid.nodes = points
        .into_iter()
        .map(|u| {
            let present = ((outgoing_vocal.sample(u) - floor) / (1. - floor)).clamp(0., 1.);
            (u, mid.sample(u) * present)
        })
        .collect();
    true
}

/// Drum-layered blend: the incoming deck plays drums alone until the handoff,
/// where its vocal and instruments arrive while the outgoing tonal material
/// releases. Only valid when both decks will have aligned stem PCM.
pub(super) fn layer(ctx: &PlanContext<'_>, plan: &mut MixPlan, handoff: f32) {
    let Some(summary) = &plan.summary else {
        return;
    };
    let n = summary.length_bars as f32;
    let ease = |v: f32| {
        let v = v.clamp(0., 1.);
        v * v * (3. - 2. * v)
    };
    // Center the outgoing vocal release on the nearest measured breath so a
    // word is never faded mid-syllable.
    let lo = (handoff - 2.).max(0.);
    let hi = (handoff + 1.).min(n);
    let mut vocal_center = handoff + 0.5;
    let mut nearest = f32::MAX;
    let steps = ((hi - lo) * 32.).ceil().max(1.) as usize;
    for i in 0..=steps {
        let u = lo + (hi - lo) * i as f32 / steps as f32;
        if crate::continuity::voice_gap(ctx.outgoing, plan.outgoing_source.sample(u)) == Some(true)
            && (u - handoff).abs() < nearest
        {
            nearest = (u - handoff).abs();
            vocal_center = u;
        }
    }
    let mut result = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    let gain = |u: f32, stem: usize, incoming: bool| {
        let center = if !incoming && stem == 0 {
            vocal_center
        } else {
            handoff + 0.5
        };
        if stem == StemKind::Bass.index() {
            let x = ease((u - handoff) / 0.25);
            if incoming { x } else { 1. - x }
        } else if stem == 1 {
            1.
        } else if incoming {
            ease((u - (handoff - 0.5)) / 2.)
        } else {
            1. - ease((u - (center - 1.)) / 2.)
        }
    };
    for i in 0..=128 {
        let u = n * i as f32 / 128.;
        for stem in 0..4 {
            result.incoming[stem].nodes.push((u, gain(u, stem, true)));
            result.outgoing[stem].nodes.push((u, gain(u, stem, false)));
        }
    }
    if let Some(last) = plan
        .clock
        .nodes
        .last()
        .map(|node| node.0)
        .filter(|last| *last > n)
    {
        for stem in 0..4 {
            result.incoming[stem]
                .nodes
                .push((last, gain(last, stem, true)));
            result.outgoing[stem]
                .nodes
                .push((last, gain(last, stem, false)));
        }
    }
    release_mid(plan, &result.outgoing[StemKind::Vocals.index()]);
    plan.requires_stems = true;
    plan.stem_mix = Some(result);
}

/// LoopOut strip-down: the looped outgoing phrase sheds its vocal first, then
/// its instruments through the exchange, while only the incoming drums play
/// until the new foreground arrives on the phrase downbeat.
pub(super) fn loop_out(plan: &mut MixPlan) {
    let Some(summary) = &plan.summary else {
        return;
    };
    let n = summary.length_bars as f32;
    let line = |nodes: &[(f32, f32)]| Polyline {
        nodes: nodes.to_vec(),
    };
    let mut mix = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::constant(1.)),
        incoming: std::array::from_fn(|_| Polyline::constant(1.)),
    };
    mix.outgoing[StemKind::Vocals.index()] = line(&[(0., 1.), (0.25, 0.), (n, 0.)]);
    mix.outgoing[StemKind::Instruments.index()] =
        line(&[(0., 1.), (1., 1.), (n / 2., 0.), (n, 0.)]);
    mix.incoming[StemKind::Vocals.index()] =
        line(&[(0., 0.), (n / 2., 0.), (n / 2. + 1., 1.), (n, 1.)]);
    mix.incoming[StemKind::Instruments.index()] = mix.incoming[StemKind::Vocals.index()].clone();
    mix.outgoing[StemKind::Bass.index()] =
        line(&[(0., 1.), (n / 2., 1.), (n / 2. + 0.25, 0.), (n, 0.)]);
    mix.incoming[StemKind::Bass.index()] =
        line(&[(0., 0.), (n / 2., 0.), (n / 2. + 0.25, 1.), (n, 1.)]);
    plan.requires_stems = true;
    plan.stem_mix = Some(mix);
}

/// Mean drum-stem level over a span of source seconds.
fn drums(t: &TrackAnalysis, start: f32, end: f32) -> f32 {
    let Some(stems) = &t.stems else { return 0. };
    let steps = 16;
    (0..steps)
        .filter_map(|i| stems.frame_at(start + (end - start) * (i as f32 + 0.5) / steps as f32))
        .map(|f| f.rms[1])
        .sum::<f32>()
        / steps as f32
}

/// The kits change on a phrase line at least two bars in and a bar before
/// the bass; a handoff too early for both gets no swap.
pub(super) fn drum_bar(handoff: f32) -> Option<f32> {
    let bar = ((handoff / 2. / 4.).floor() * 4.).max(2.);
    (bar <= handoff - 1.).then_some(bar)
}

#[cfg(test)]
#[test]
fn drum_swaps_sit_on_a_phrase_line_before_the_bass() {
    assert_eq!(drum_bar(16.), Some(8.));
    assert_eq!(drum_bar(24.), Some(12.));
    assert_eq!(drum_bar(4.), Some(2.));
    assert_eq!(drum_bar(3.), Some(2.));
    assert_eq!(drum_bar(2.5), None);
    assert_eq!(drum_bar(1.), None);
}

/// Both decks carry a real drum part where the swap happens, so taking the
/// outgoing kit away reveals a groove instead of a gap.
pub(super) fn swap_supported(
    plan: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    at: f32,
) -> bool {
    let window = |line: &Polyline| (line.sample((at - 1.).max(0.)), line.sample(at + 1.));
    let (a0, a1) = window(&plan.outgoing_source);
    let (b0, b1) = window(&plan.incoming_source);
    drums(a, a0, a1) > 0.01 && drums(b, b0, b1) > 0.01
}

/// Predicted master level of a plan at bar `u`, from the shared band model.
fn predicted_level(plan: &MixPlan, a: &TrackAnalysis, b: &TrackAnalysis, u: f32) -> f32 {
    crate::level::at(plan, a, b, u)
}

/// A swap must also start from the level the plain blend would have had.
fn entry_level_ok(
    blend: &MixPlan,
    swapped: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
) -> bool {
    predicted_level(swapped, a, b, 0.) >= predicted_level(blend, a, b, 0.) - 3.
}

/// A swap moves parts one at a time, so a fading outgoing part or a gap in
/// an incoming one can leave a hole the plain blend would have filled. Keep
/// the swap only where it never sits clearly below the blend it replaces.
pub(super) fn swap_holds_level(
    blend: &MixPlan,
    swapped: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    n: f32,
) -> bool {
    entry_level_ok(blend, swapped, a, b)
        && (0..=(n * 2.) as usize).all(|i| {
            let u = i as f32 * 0.5;
            predicted_level(swapped, a, b, u) >= predicted_level(blend, a, b, u) - 6.
        })
}

/// The outgoing vocal stem rides alone over the incoming instrumental until
/// its line ends. Requires an incoming deck with no voice in the window.
pub(super) fn ride_supported(
    plan: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    n: f32,
) -> Option<f32> {
    crate::ride::fit(
        a,
        b,
        &plan.outgoing_source,
        &plan.incoming_source,
        n,
        true,
        plan.lanes.pitch_a.sample(0.),
        plan.lanes.pitch_b.sample(0.),
    )?;
    let incoming = &plan.incoming_source;
    let quiet = (0..64).all(|i| {
        let u = n * (i as f32 + 0.5) / 64.;
        crate::continuity::voice_active(b, incoming.sample(u)) < 0.25
    });
    if !quiet {
        return None;
    }
    // The last measured breath in A's final quarter, so the word completes.
    (0..=64)
        .rev()
        .map(|i| n * 0.75 + (n * 0.25 - 0.25) * i as f32 / 64.)
        .find(|u| crate::continuity::voice_gap(a, plan.outgoing_source.sample(*u)) == Some(true))
        .filter(|_| crate::continuity::voice_active(a, plan.outgoing_source.sample(n * 0.6)) > 0.35)
}

/// When keys cannot be matched, the drum layer may continue, but incoming
/// pitched parts wait until *all* outgoing pitched parts have left. Merely
/// crossfading each stem separately still puts the old voice over the new bass.
pub(super) fn separate_tonal(plan: &mut MixPlan) -> Option<()> {
    let n = plan.summary.as_ref()?.length_bars as f32;
    let Some(stems) = plan.stem_mix.as_mut() else {
        return Some(());
    };
    let pitched = [
        StemKind::Vocals.index(),
        StemKind::Bass.index(),
        StemKind::Instruments.index(),
    ];
    // An independently fitted bridge can already separate every pitched
    // part. Do not add another fade and change its predicted energy.
    let mut points: Vec<f32> = stems
        .outgoing
        .iter()
        .chain(&stems.incoming)
        .flat_map(|lane| lane.nodes.iter().map(|p| p.0))
        .collect();
    points.sort_by(f32::total_cmp);
    points.dedup();
    let checks = points
        .iter()
        .copied()
        .chain(points.windows(2).map(|w| (w[0] + w[1]) * 0.5));
    if checks.into_iter().all(|u| {
        let a = pitched
            .iter()
            .map(|s| stems.outgoing[*s].sample(u))
            .fold(0., f32::max);
        let b = pitched
            .iter()
            .map(|s| stems.incoming[*s].sample(u))
            .fold(0., f32::max);
        a * b <= 0.0001
    }) {
        return Some(());
    }
    let release = pitched
        .iter()
        .map(|s| {
            let lane = &stems.outgoing[*s];
            lane.nodes
                .windows(2)
                .filter(|p| p[0].1 > 0.0001)
                .map(|p| p[1].0)
                .fold(0., f32::max)
        })
        .fold(0., f32::max);
    if release + 0.25 > n {
        return None;
    }
    for stem in pitched {
        let original = stems.incoming[stem].clone();
        let mut nodes: Vec<f32> = original.nodes.iter().map(|p| p.0).collect();
        nodes.extend([release, release + 0.25, n]);
        nodes.sort_by(f32::total_cmp);
        nodes.dedup();
        stems.incoming[stem].nodes = nodes
            .into_iter()
            .map(|u| {
                let x = ((u - release) / 0.25).clamp(0., 1.);
                (u, original.sample(u) * x * x * (3. - 2. * x))
            })
            .collect();
    }
    Some(())
}

/// Where the voices change hands. The outgoing voice leaves on a measured
/// release at or after `from`, so no word is cut; the incoming voice enters
/// on its own breath after that. Both moves finish before the plan ends,
/// since stem gains hold their last value once it does.
pub(super) fn voice_points(
    plan: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    from: f32,
    n: f32,
) -> Option<(f32, f32)> {
    let steps = |lo: f32, hi: f32| (0..=32).map(move |i| lo + (hi - lo) * i as f32 / 32.);
    if from > n - 1.5 {
        return None;
    }
    let out = steps(from, n - 1.5)
        .find(|u| crate::continuity::voice_released(a, plan.outgoing_source.sample(*u)))?;
    steps(out + 0.25, n - 0.75)
        .find(|u| {
            let sec = plan.incoming_source.sample(*u);
            crate::continuity::voice_gap(b, sec) == Some(true)
                || crate::continuity::voice_active(b, sec) < 0.25
        })
        .map(|entry| (out, entry))
}

/// Stem swap in phrase steps: the incoming drums join for a bar and replace
/// the outgoing kit on `drum_bar`, the bass changes hands on `handoff`, then
/// the tonal layers cross over. `voice` is from [`voice_points`]; with `ride`
/// the outgoing vocal stays on top of the incoming instrumental until then.
/// Every move must end inside the plan: `handoff + 2.25 <= n`.
pub(super) fn swap(plan: &mut MixPlan, handoff: f32, drum_bar: f32, voice: (f32, f32), ride: bool) {
    let Some(summary) = &plan.summary else { return };
    let n = summary.length_bars as f32;
    let ease = |v: f32| {
        let v = v.clamp(0., 1.);
        v * v * (3. - 2. * v)
    };
    let (voice_out, voice_in) = voice;
    let gain = |u: f32, stem: usize, incoming: bool| -> f32 {
        match (stem, incoming) {
            (1, true) => ease((u - (drum_bar - 1.)) / 1.),
            (1, false) => 1. - ease((u - drum_bar) / 0.25),
            (3, true) => ease((u - handoff) / 0.25),
            (3, false) => 1. - ease((u - handoff) / 0.25),
            (2, true) => ease((u - (handoff - 0.5)) / 2.),
            (2, false) => 1. - ease((u - handoff) / 2.),
            (0, false) => 1. - ease((u - voice_out) / 0.25),
            _ => ease((u - voice_in) / 0.5),
        }
    };
    let last = plan.clock.nodes.last().map_or(n, |p| p.0).max(n);
    let mut result = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    for i in 0..=256 {
        let u = n * i as f32 / 256.;
        for stem in 0..4 {
            result.outgoing[stem].nodes.push((u, gain(u, stem, false)));
            result.incoming[stem].nodes.push((u, gain(u, stem, true)));
        }
    }
    // After the plan the incoming deck plays complete.
    for stem in 0..4 {
        result.incoming[stem].nodes.push((last.max(n + 0.01), 1.));
        result.outgoing[stem].nodes.push((last.max(n + 0.01), 0.));
    }
    // The stems now carry every handover, so the mixer must stay out of the
    // way: open the fader to centre while B is still silent, compensate each
    // deck to unity power, and close to B only once A's stems are out.
    let outgoing_done = (handoff + 2.).max(voice_out + 0.25);
    let close = (n - 0.5).max(outgoing_done).min(n - 0.125);
    let trim = plan.lanes.gain_b.nodes.last().map_or(0., |p| p.1);
    let to_db = |level: f32| (-20. * level.max(0.0001).log10()).min(12.);
    let lanes = &mut plan.lanes;
    lanes.xfader.nodes.clear();
    lanes.gain_a.nodes.clear();
    lanes.gain_b.nodes.clear();
    for j in 0..=(n * 64.) as usize {
        let u = j as f32 / 64.;
        let x = if u < 0.25 {
            -1. + ease(u / 0.25)
        } else if u < close {
            0.
        } else {
            ease((u - close) / (n - close))
        };
        // The engine's equal-power law: square roots of the fader travel.
        let t = (x + 1.) * 0.5;
        lanes.xfader.nodes.push((u, x));
        let outgoing = if u >= n { -96. } else { to_db((1. - t).sqrt()) };
        lanes.gain_a.nodes.push((u, outgoing));
        lanes
            .gain_b
            .nodes
            .push((u, (trim + to_db(t.sqrt())).clamp(-96., 12.)));
    }
    lanes.gain_a.nodes.push((n, -96.));
    lanes.gain_a.nodes.dedup_by(|x, y| x.0 <= y.0);
    for eq in [&mut lanes.eq_a, &mut lanes.eq_b] {
        eq.low = Polyline::constant(0.);
        eq.mid = Polyline::constant(0.);
        eq.high = Polyline::constant(0.);
    }
    if ride {
        // Throw the last word into a short echo as the voice closes.
        let end = voice_out;
        let word = (end - 0.25).max(handoff);
        lanes.fx_send_a = Polyline {
            nodes: vec![
                (0., 0.),
                (word, 0.),
                (end + 0.25, 0.25),
                ((end + 0.75).min(n), 0.),
            ],
        };
        lanes.fx_send_a.nodes.dedup_by(|x, y| x.0 <= y.0);
    }
    plan.requires_stems = true;
    plan.stem_mix = Some(result);
    plan.stages = vec![
        mixless_protocol::MixStage {
            start_bar: 0.,
            end_bar: drum_bar,
            label: "Incoming drums join".into(),
        },
        mixless_protocol::MixStage {
            start_bar: drum_bar,
            end_bar: handoff,
            label: "Drum swap".into(),
        },
        mixless_protocol::MixStage {
            start_bar: handoff,
            end_bar: if ride { handoff + 2. } else { n },
            label: "Bass swap and tonal crossover".into(),
        },
    ];
    if ride {
        plan.stages.push(mixless_protocol::MixStage {
            start_bar: handoff + 2.,
            end_bar: voice_out.max(handoff + 2.),
            label: "Outgoing vocal over incoming".into(),
        });
        plan.stages.push(mixless_protocol::MixStage {
            start_bar: voice_out.max(handoff + 2.),
            end_bar: n,
            label: "Bass swap and tonal crossover".into(),
        });
    }
    plan.stages.retain(|s| s.end_bar > s.start_bar);
}

/// Vocal carry: the outgoing voice keeps singing over the incoming deck's
/// instrumental until its line ends with the plan. The incoming parts arrive
/// one at a time over the first phrase while the outgoing ones leave at equal
/// power: the kit first, the bass on a bar line, the tonal layers across the
/// whole phrase. The voice settles to the incoming bed's level and leaves the
/// way [`crate::ride::shape`] chose. The incoming voice waits until the
/// outgoing line has finished, so two foregrounds never overlap.
///
/// The window itself must already end at the outgoing vocal's release — this
/// shapes the envelopes inside that window and never extends the plan.
pub(super) fn carry(plan: &mut MixPlan, shape: &crate::ride::Shape, bpm: f32) {
    use crate::ride::Exit;
    let Some(summary) = &plan.summary else { return };
    let n = summary.length_bars as f32;
    let ease = |v: f32| {
        let v = v.clamp(0., 1.);
        v * v * (3. - 2. * v)
    };
    let quarter = std::f32::consts::FRAC_PI_2;
    let leave = shape.leave.clamp(1., (n - 1.).max(1.));
    // The kits change on the bar line halfway through the phrase, the bass
    // right after them, so one groove and one bass line sound at a time.
    let swap = (leave / 2.).round().max(1.);
    let voice_end = shape.voice_end.clamp(leave + 0.5, n);
    let sit = 10f32.powf(shape.sit_db / 20.);
    // Every fade finishes before the fader closes, so the closing move only
    // ever acts on silence.
    let gone = (n - 0.25).max(leave + 0.25);
    let (fade_from, fade_to) = match shape.exit {
        Exit::Tail => (voice_end - 0.5, voice_end + 0.5),
        Exit::Throw => (voice_end - 0.0625, voice_end),
        Exit::Sweep => (voice_end - 0.25, voice_end + 0.25),
        Exit::Wash => (voice_end - 2., voice_end + 0.5),
    };
    let (fade_to, fade_from) = (fade_to.min(gone), fade_from.min(gone - 0.0625));
    let voice = |u: f32| -> f32 {
        let level = 1. + (sit - 1.) * ease(u / leave);
        level * (1. - ease((u - fade_from) / (fade_to - fade_from).max(0.0625)))
    };
    let gain = |u: f32, stem: usize, incoming: bool| -> f32 {
        let phrase = ease(u / leave);
        match (stem, incoming) {
            (0, false) => voice(u),
            // The incoming voice comes in once the line has gone, swelling
            // over the last half bar so a held note never cuts in.
            (0, true) => ease((u - fade_to.min(n - 0.5)) / 0.5),
            // Drums: the incoming kit builds under the outgoing one over the
            // bars before the swap, and the outgoing kit steps out on it.
            (1, false) => 1. - ease((u - (swap - 0.5)) / 0.5),
            (1, true) => ease((u - swap / 2.) / (swap / 2.)),
            // Bass changes hands on the bar line, never doubled.
            (3, false) => 1. - ease((u - (swap - 0.125)) / 0.125),
            (3, true) => ease((u - swap) / 0.125),
            // Tonal layers cross over the phrase at equal power.
            (_, false) => (phrase * quarter).cos(),
            (_, true) => (phrase * quarter).sin(),
        }
    };
    let last = plan.clock.nodes.last().map_or(n, |p| p.0).max(n);
    let mut result = StemMix {
        outgoing: std::array::from_fn(|_| Polyline::default()),
        incoming: std::array::from_fn(|_| Polyline::default()),
    };
    for i in 0..=512 {
        let u = n * i as f32 / 512.;
        for stem in 0..4 {
            result.outgoing[stem].nodes.push((u, gain(u, stem, false)));
            result.incoming[stem].nodes.push((u, gain(u, stem, true)));
        }
    }
    for stem in 0..4 {
        result.incoming[stem].nodes.push((last.max(n + 0.01), 1.));
        result.outgoing[stem].nodes.push((last.max(n + 0.01), 0.));
    }
    // The stems do the mixing: both decks sit at unity while the voice
    // rides, and the fader only closes once the outgoing deck is silent.
    let trim = plan.lanes.gain_b.nodes.last().map_or(0., |p| p.1);
    let to_db = |level: f32| (-20. * level.max(0.0001).log10()).min(12.);
    let lanes = &mut plan.lanes;
    lanes.xfader.nodes.clear();
    lanes.gain_a.nodes.clear();
    lanes.gain_b.nodes.clear();
    // The incoming deck is ridden up from the outgoing level over the
    // arrival and the phrase after it.
    let ride_up = (leave + 8.).min(fade_from).max(leave);
    let lift = |u: f32| -shape.entry_db * (1. - ease(u / ride_up));
    for j in 0..=(n * 64.) as usize {
        let u = j as f32 / 64.;
        let x = if u < 0.125 {
            -1. + ease(u / 0.125)
        } else if u < gone {
            0.
        } else {
            ease((u - gone) / (n - gone).max(0.0625))
        };
        let t = (x + 1.) * 0.5;
        lanes.xfader.nodes.push((u, x));
        let outgoing = if u >= n { -96. } else { to_db((1. - t).sqrt()) };
        lanes.gain_a.nodes.push((u, outgoing));
        lanes
            .gain_b
            .nodes
            .push((u, (trim + to_db(t.sqrt()) + lift(u)).clamp(-96., 12.)));
    }
    lanes.gain_a.nodes.push((n, -96.));
    lanes.gain_a.nodes.dedup_by(|x, y| x.0 <= y.0);
    for eq in [&mut lanes.eq_a, &mut lanes.eq_b] {
        eq.low = Polyline::constant(0.);
        eq.mid = Polyline::constant(0.);
        eq.high = Polyline::constant(0.);
    }
    // Once the outgoing low end has gone the voice keeps only its own body:
    // a gentle high-pass keeps it off the incoming bass.
    let body = 150.;
    let mut hp = vec![(0., 20.), (leave, body)];
    let mut lp = vec![(0., 20000.)];
    let mut send = vec![(0., 0.)];
    let fx = match shape.exit {
        Exit::Tail => {
            send.extend([(fade_from, 0.), (voice_end.min(gone), 0.18), (fade_to, 0.)]);
            crate::send_fx::reverb(2.)
        }
        Exit::Throw => {
            // The dry voice stops with the word; the echo carries it on.
            hp.extend([(voice_end - 0.25, body), (voice_end, 600.)]);
            send.extend([
                (voice_end - 0.25, 0.),
                (voice_end - 0.0625, 0.35),
                (voice_end + 0.0625, 0.),
            ]);
            crate::send_fx::echo(mixless_protocol::FxKind::LowCutEcho, 0.75, 0.5, bpm)
        }
        Exit::Sweep => {
            // Thin the voice upwards over two bars so the drop lands clean.
            let from = (voice_end - 2.).max(leave);
            hp.extend([(from, body), (voice_end, 2500.)]);
            send.extend([(from, 0.), (voice_end, 0.3), (fade_to, 0.)]);
            crate::send_fx::echo(mixless_protocol::FxKind::LowCutEcho, 0.5, 0.3, bpm)
        }
        Exit::Wash => {
            // Push the voice back into a long room while it fades.
            lp.extend([(fade_from.max(leave), 20000.), (fade_to, 3000.)]);
            send.extend([
                (fade_from.max(leave), 0.),
                (voice_end.min(gone), 0.4),
                (n, 0.),
            ]);
            crate::send_fx::reverb(3.5)
        }
    };
    let line = |mut nodes: Vec<(f32, f32)>, rest: f32| {
        nodes.retain(|p| p.0 <= n);
        nodes.sort_by(|x, y| x.0.total_cmp(&y.0));
        nodes.dedup_by(|x, y| x.0 <= y.0);
        nodes.push((n, rest));
        nodes.dedup_by(|x, y| x.0 <= y.0);
        Polyline { nodes }
    };
    // The outgoing deck is silent by `n`; its filter returns to neutral there
    // so the next pair starts from a clean channel.
    lanes.filter_a.hp_hz = line(hp, 20.);
    lanes.filter_a.lp_hz = line(lp, 20000.);
    lanes.fx_send_a = line(send, 0.);
    plan.send_fx = Some(fx);
    plan.requires_stems = true;
    plan.stem_mix = Some(result);
    let exit = match shape.exit {
        Exit::Tail => "Vocal fades out",
        Exit::Throw => "Vocal echo throw",
        Exit::Sweep => "Vocal filter sweep",
        Exit::Wash => "Vocal reverb wash",
    };
    plan.stages = vec![
        mixless_protocol::MixStage {
            start_bar: 0.,
            end_bar: leave,
            label: "Incoming parts arrive".into(),
        },
        mixless_protocol::MixStage {
            start_bar: leave,
            end_bar: fade_from.max(leave),
            label: "Outgoing vocal over incoming".into(),
        },
        mixless_protocol::MixStage {
            start_bar: fade_from.max(leave),
            end_bar: n,
            label: exit.into(),
        },
    ];
    plan.stages.retain(|s| s.end_bar > s.start_bar);
}
