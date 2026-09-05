//! Conservative DJ policy: musical safety gates precede ranking. A scalar
//! score cannot compensate for clashing foregrounds or an unreliable beat grid.
use crate::{
    candidates::candidates,
    constraints::{covers_user_range, user_range},
    grid::Grid,
    score::{key_match, section},
    PlanContext, PlannerOptions,
};
use mixless_protocol::{
    AutomationLanes, BarFeature, BarMap, EqLane, FilterLane, MixPlan, MixPlanSummary, Polyline,
    StrategyId as S, TrackAnalysis, TransitionMode,
};

const MAX_LOG_TEMPO_PER_SEC: f32 = 0.0035;
const KILL: f32 = -96.;
fn ease(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (10. + t * (-15. + 6. * t))
}
fn line(nodes: &[(f32, f32)]) -> Polyline {
    Polyline {
        nodes: nodes.to_vec(),
    }
}
fn feature(track: &TrackAnalysis, sec: f32) -> Option<&BarFeature> {
    let i = track.bars.partition_point(|b| b.end_sec <= sec);
    track.bars.get(i).filter(|b| b.start_sec <= sec)
}
fn neutral() -> AutomationLanes {
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
    }
}
fn grid_reliable(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    if t.tempo.beats.len() < 8 || t.tempo.downbeats.len() < 2 {
        return false;
    }
    let grid = Grid(t);
    let first = grid.beat(start).floor() as usize;
    let last = grid.beat(end).ceil() as usize;
    if last >= t.tempo.beats.len() {
        return false;
    }
    (first..last).all(|i| {
        let expected = 60. / grid.bpm(i as f32);
        let actual = t.tempo.beats[i + 1] - t.tempo.beats[i];
        (actual / expected - 1.).abs() < 0.025
            && t.tempo.segments.iter().any(|s| {
                i as f32 >= s.start_beat && (i as f32) < s.end_beat && s.confidence >= 0.65
            })
    })
}
fn average_rms(t: &TrackAnalysis, start: f32, end: f32) -> Option<f32> {
    let mut sum = 0.;
    let mut count = 0.;
    for b in &t.bars {
        if b.end_sec > start && b.start_sec < end && b.rms.is_finite() && b.rms > 0.0001 {
            sum += b.rms * b.rms;
            count += 1.;
        }
    }
    (count > 0.).then(|| (sum / count).sqrt())
}
fn audible_end(t: &TrackAnalysis) -> f32 {
    t.bars
        .iter()
        .rev()
        .find(|b| b.rms > 0.001)
        .map_or(t.duration_sec, |b| b.end_sec.min(t.duration_sec))
}
fn points(t: &TrackAnalysis, cues: &[mixless_protocol::Cue], out: bool) -> Vec<f32> {
    let g = Grid(t);
    let mut result = candidates(t, cues, out);
    if out {
        result.insert(0, g.floor_bar(audible_end(t)));
    } else if let Some(first) = t.bars.iter().find(|b| b.rms > 0.001) {
        result.insert(0, g.ceil_bar(first.start_sec));
    }
    // Add low-vocal phrase boundaries near beginning/end. Do not search arbitrary
    // mid-verse points just because their BPM happens to fit.
    for b in &t.bars {
        let relative = b.start_sec / t.duration_sec;
        if b.bar_index % 8 == 0
            && b.rms > 0.001
            && b.vocal_presence < 0.45
            && if out {
                relative > 0.55
            } else {
                relative < 0.35
            }
        {
            result.push(g.floor_bar(b.start_sec));
        }
    }
    result.retain(|beat| g.sec(*beat) >= 0. && g.sec(*beat) <= audible_end(t) + 0.001);
    result.sort_by(|a, b| {
        let cost = |beat: f32| {
            let sec = g.sec(beat);
            let vocal = feature(t, (sec - 0.01).max(0.)).map_or(0.6, |b| b.vocal_presence);
            let position = if out {
                1. - sec / t.duration_sec
            } else {
                sec / t.duration_sec
            };
            vocal + position * 0.4
        };
        cost(*a).total_cmp(&cost(*b))
    });
    // Hard cue anchors are always included ahead of the top-K soft candidates.
    if let Some((start, end)) = user_range(cues, t.sample_rate) {
        result.insert(
            0,
            if out {
                g.ceil_bar(end)
            } else {
                g.floor_bar(start)
            },
        );
    }
    let mut unique = Vec::new();
    for beat in result {
        if !unique.iter().any(|v: &f32| (*v - beat).abs() < 0.01) {
            unique.push(beat);
        }
    }
    unique.truncate(8);
    unique
}

pub(crate) fn plan(ctx: &PlanContext<'_>, options: &PlannerOptions) -> MixPlan {
    let a = ctx.outgoing;
    let b = ctx.incoming;
    let outs = points(a, ctx.cues_out, true);
    let ins = points(b, ctx.cues_in, false);
    let mut best: Option<MixPlan> = None;
    for &out in &outs {
        for &input in &ins {
            for mode in [TransitionMode::BeatBlend, TransitionMode::PhraseBridge] {
                if let Some(p) = pair(ctx, options, out, input, mode) {
                    if best.as_ref().is_none_or(|old| {
                        p.summary.as_ref().unwrap().score > old.summary.as_ref().unwrap().score
                    }) {
                        best = Some(p);
                    }
                }
            }
        }
    }
    best.unwrap_or_else(||MixPlan{failure_reason:Some("No smooth transition fits the remaining audio and user cue ranges; choose another mix point or track".into()),..Default::default()})
}

fn pair(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    out: f32,
    input: f32,
    mode: TransitionMode,
) -> Option<MixPlan> {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let ga = Grid(a);
    let gb = Grid(b);
    let outgoing_bpm = ga.bpm((out - 0.01).max(0.)) * ctx.offset_a.rate;
    let incoming_bpm = gb.bpm(input) * ctx.offset_b.rate;
    let ratio = outgoing_bpm / incoming_bpm;
    let factor = if (ratio / 2. - 1.).abs() < 0.08 {
        0.5
    } else if (ratio / 0.5 - 1.).abs() < 0.08 {
        2.
    } else {
        1.
    };
    let map = if factor == 1. {
        BarMap::OneToOne
    } else {
        BarMap::TwoToOne {
            outgoing_is_double: factor == 0.5,
        }
    };
    let (key, shift) = key_match(
        a,
        b,
        ctx.offset_a.pitch_semitones,
        ctx.offset_b.pitch_semitones,
    );
    let blend = mode == TransitionMode::BeatBlend;
    let hold = options.strategy == Some(S::EnergyHold) && blend;
    let correction = outgoing_bpm * factor / incoming_bpm;
    if blend
        && ((correction - 1.).abs() > 0.08
            || shift != 0.
            || key < 0.85
            || a.key_confidence < 0.5
            || b.key_confidence < 0.5)
    {
        return None;
    }
    // At maximum slope a quintic ramp moves 1.875x faster than its average.
    let mut n = if blend { 16u16 } else { 4 };
    if blend
        && correction.ln().abs() * 1.875 / (16. * ga.meter() * 60. / outgoing_bpm)
            > MAX_LOG_TEMPO_PER_SEC
    {
        n = 32;
    }
    if let Some((min, _)) = user_range(ctx.cues_out, a.sample_rate) {
        n = n.max(((out - ga.floor_bar(min)) / ga.meter()).ceil().max(1.) as u16);
    }
    if blend {
        if let Some((_, max)) = user_range(ctx.cues_in, b.sample_rate) {
            n = n.max(
                ((gb.ceil_bar(max) - input) / (ga.meter() * factor))
                    .ceil()
                    .max(1.) as u16,
            );
        }
    }
    let available = ((out - ga.ceil_bar(options.earliest_outgoing_sec.max(0.))) / ga.meter()
        + 0.00001)
        .floor()
        .max(0.) as u16;
    if !blend {
        n = n.min(available);
    }
    if n == 0 || n > available || n > 64 {
        return None;
    }
    let n = n as f32;
    let start_beat = out - n * ga.meter();
    let start = ga.sec(start_beat);
    let end = ga.sec(out);
    let bin = gb.sec(input);
    if feature(b, bin + 0.001).is_some_and(|bar| bar.rms <= 0.001)
        || feature(a, end - 0.001).is_some_and(|bar| bar.rms <= 0.001)
    {
        return None;
    }
    let bpm_start = ga.bpm(start_beat) * ctx.offset_a.rate;
    if end > a.duration_sec + 0.001
        || start < options.earliest_outgoing_sec
        || bin >= b.duration_sec
    {
        return None;
    }
    if !covers_user_range(start, end, ctx.cues_out, a.sample_rate) {
        return None;
    }
    let mut lanes = neutral();
    lanes.rate_a.nodes.clear();
    lanes.rate_b.nodes.clear();
    lanes.pitch_a = Polyline::constant(ctx.offset_a.pitch_semitones);
    lanes.pitch_b = Polyline::constant(ctx.offset_b.pitch_semitones);
    let mut clock = Polyline::default();
    let mut source_a = Polyline::default();
    let mut source_b = Polyline::default();
    let mut master = Polyline::default();
    let mut wall = 0f64;
    let mut prev_a = start as f64;
    let mut prev_rate = ctx.offset_a.rate as f64;
    let mut max_slope = 0f32;
    let mut prev_log = bpm_start.ln();
    // A bridge replaces the next phrase's downbeat. Starting B on A's last
    // beat anticipates that downbeat and sounds like a one-beat phrase error.
    let start_b = if blend { 0. } else { n };
    for j in 0..=n as usize * 64 {
        let u = j as f32 / 64.;
        let ba = start_beat + u * ga.meter();
        let ta = ga.sec(ba);
        let target = if hold {
            bpm_start
        } else {
            incoming_bpm / factor
        };
        let bpm = if blend {
            (bpm_start.ln() + (target / bpm_start).ln() * ease(u / n)).exp()
        } else {
            ga.bpm(ba) * ctx.offset_a.rate
        };
        let ra = if blend {
            bpm / ga.bpm(ba)
        } else {
            ctx.offset_a.rate
        };
        if !(0.88..=1.12).contains(&(ra / ctx.offset_a.rate)) {
            return None;
        }
        if j > 0 {
            let dt = 2. * (ta as f64 - prev_a) / (ra as f64 + prev_rate);
            if dt <= 0. {
                return None;
            }
            max_slope = max_slope.max((bpm.ln() - prev_log).abs() / dt as f32);
            wall += dt;
        }
        clock.nodes.push((u, wall as f32));
        source_a.nodes.push((u, ta));
        master.nodes.push((u, bpm));
        lanes.rate_a.nodes.push((u, ra));
        if blend {
            let bb = input + u * ga.meter() * factor;
            source_b.nodes.push((u, gb.sec(bb)));
            let rb = bpm * factor / gb.bpm(bb);
            if !(0.5..=2.).contains(&ra)
                || !(0.5..=2.).contains(&rb)
                || !(0.88..=1.12).contains(&(rb / ctx.offset_b.rate))
            {
                return None;
            }
            lanes.rate_b.nodes.push((u, rb));
        }
        prev_a = ta as f64;
        prev_rate = ra as f64;
        prev_log = bpm.ln();
    }
    if blend && max_slope > MAX_LOG_TEMPO_PER_SEC * 1.02 {
        return None;
    }
    // Keep the plan alive for one incoming beat after the cut. This lets the
    // engine launch B exactly at n even when A finishes at the file boundary.
    let bridge_tail = 60. / incoming_bpm;
    let b_end = if blend {
        gb.sec(input + n * ga.meter() * factor)
    } else {
        bin + bridge_tail * ctx.offset_b.rate
    };
    if b_end > b.duration_sec + 0.001 || !covers_user_range(bin, b_end, ctx.cues_in, b.sample_rate)
    {
        return None;
    }
    if !blend {
        lanes.rate_b = Polyline::constant(ctx.offset_b.rate);
        source_b = Polyline::default();
        clock.nodes.push((n + 0.25, wall as f32 + bridge_tail));
    }
    if blend && (!grid_reliable(a, start, end) || !grid_reliable(b, bin, b_end)) {
        return None;
    }
    let mut clash = 0.;
    let mut vocal_a = 0.;
    let mut vocal_b = 0.;
    let samples = (n * ga.meter() * 4.) as usize;
    for j in 0..samples {
        let t = (j as f32 + 0.5) / samples as f32;
        let (ta, tb) = if blend {
            (source_a.sample(n * t), source_b.sample(n * t))
        } else {
            // Only compare the foregrounds at the actual bridge boundary.
            (end - (1. - t) * 60. / outgoing_bpm, bin + t * (b_end - bin))
        };
        let va = feature(a, ta).map_or(1., |f| f.vocal_presence);
        let vb = feature(b, tb).map_or(1., |f| f.vocal_presence);
        vocal_a += va / samples as f32;
        vocal_b += vb / samples as f32;
        if va > 0.5 && vb > 0.5 {
            if section(a, ta) == mixless_protocol::SectionLabel::Verse
                && section(b, tb) == mixless_protocol::SectionLabel::Chorus
            {
                return None;
            }
            clash += 1. / samples as f32;
        }
    }
    if blend && clash > 0.0625 {
        return None;
    }
    let rms_a = average_rms(
        a,
        (end - 4. * ga.meter() * 60. / outgoing_bpm).max(start),
        end,
    );
    let rms_b = average_rms(
        b,
        bin,
        (bin + 4. * gb.meter() * 60. / incoming_bpm).min(b.duration_sec),
    );
    // Match local program level within 6 dB, reserving at least 3 dB source
    // peak headroom. Unknown peaks never authorize positive gain.
    let peak_b = b
        .bars
        .iter()
        .filter(|bar| bar.end_sec > bin && bar.start_sec < (bin + 8.).min(b.duration_sec))
        .map(|bar| bar.rms * bar.crest)
        .filter(|peak| peak.is_finite())
        .fold(0., f32::max);
    let headroom = if peak_b > 0. {
        (20. * (0.707 / peak_b).log10()).clamp(0., 6.)
    } else {
        0.
    };
    let trim_b = match (rms_a, rms_b) {
        (Some(a), Some(b)) => (20. * (a / b).log10()).clamp(-6., headroom),
        _ => 0.,
    };
    if blend {
        let middle = (n / 2.).floor();
        let width = 0.5 / ga.meter();
        // One bass foreground until the phrase downbeat. The old -96 dB ramp
        // spread across 8 bars created a long bass hole; exchange within one beat.
        lanes.xfader = line(&[
            (0., -1.),
            (n * 0.25, -0.55),
            (middle, 0.),
            (n * 0.75, 0.55),
            (n, 1.),
        ]);
        lanes.gain_a = line(&[(0., 0.), (n - 0.001, 0.), (n, KILL)]);
        lanes.gain_b = Polyline::constant(trim_b);
        lanes.eq_a.low.nodes.clear();
        lanes.eq_b.low.nodes.clear();
        for j in 0..=n as usize * 64 {
            let u = j as f32 / 64.;
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
    } else {
        let handoff = start_b;
        // Finish the dry fade just before the downbeat, then open B fully on
        // its first kick. Filtered echo carries the short gap, with no overlap
        // of independent drum grids or incompatible melodic foregrounds.
        let fade = (0.04 * outgoing_bpm / (60. * ga.meter())).min(0.125);
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
        lanes.filter_a.lp_hz = line(&[
            (0., 20000.),
            ((handoff - 0.5).max(0.), 20000.),
            (handoff - 0.25, 2000.),
            (handoff, 1200.),
        ]);
        lanes.filter_a.lp_hz.nodes.dedup_by(|a, b| a.0 == b.0);
        lanes.fx_send_a = line(&[
            (0., 0.),
            ((handoff - 0.5).max(0.), 0.),
            (handoff - 0.25, 0.18),
            (handoff, 0.),
        ]);
        lanes.fx_send_a.nodes.dedup_by(|a, b| a.0 == b.0);
    }
    // A never jumps on the first sample; B's pitch is set while inaudible and
    // held thereafter. A key change is made by handing over musical material.
    let ending = mixless_protocol::PerformanceOffset {
        rate: lanes.rate_b.sample(n),
        pitch_semitones: ctx.offset_b.pitch_semitones,
    };
    let structural = matches!(
        section(a, end - 0.001),
        mixless_protocol::SectionLabel::Outro
            | mixless_protocol::SectionLabel::Break
            | mixless_protocol::SectionLabel::Drop
            | mixless_protocol::SectionLabel::Chorus
    );
    let position = 0.03 * end / a.duration_sec - 0.03 * bin / b.duration_sec;
    let quality = if blend {
        0.78 + 0.1 * key - 0.1 * clash
    } else {
        0.48 + 0.08 * (1. - vocal_a.min(vocal_b))
    };
    Some(MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: if blend {
                if hold {
                    S::EnergyHold
                } else {
                    S::BassSwap
                }
            } else {
                S::EchoOut
            },
            score: (quality + position + if structural { 0.03 } else { 0. }).clamp(0., 1.),
            used_fallback: !blend,
            length_bars: n as u16,
        }),
        incoming_offset_end: ending,
        outgoing_offset: ctx.offset_a,
        bar_map: if blend { map } else { BarMap::OneToOne },
        t_in_a: start,
        t_out_a: end,
        t_in_b: bin,
        t_end_b: b_end,
        clock,
        incoming_source: source_b,
        outgoing_source: source_a,
        incoming_start_bar: start_b,
        transition_mode: Some(mode),
        master_bpm: master,
        lanes,
        handoff_bar: Some(if blend { n / 2. } else { start_b }),
        literal_half_double: false,
        failure_reason: None,
    })
}
