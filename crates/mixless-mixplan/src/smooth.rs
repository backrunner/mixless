//! Conservative DJ policy: musical safety gates precede ranking. A scalar
//! score cannot compensate for clashing foregrounds or an unreliable beat grid.
use crate::{
    constraints::{covers_user_range, user_range},
    grid::Grid,
    musical::{average_rms, bass_handoff, feature, incoming_trim, percussion_only},
    phrasing::{boundary_quality, points},
    policy::{self, Evidence as FxEvidence, Technique},
    score::{key_match, section},
    PlanContext, PlannerOptions,
};
use mixless_protocol::{
    AutomationLanes, BarMap, EqLane, FilterLane, MixPlan, MixPlanSummary, Polyline, ScratchOp,
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
        scratch_a: None,
        scratch_b: None,
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
pub(crate) fn plan(ctx: &PlanContext<'_>, options: &PlannerOptions) -> MixPlan {
    let a = ctx.outgoing;
    let b = ctx.incoming;
    let outs = points(a, ctx.cues_out, true, options.earliest_outgoing_sec);
    let ins = points(b, ctx.cues_in, false, 0.);
    let mut best: Option<MixPlan> = None;
    for &out in &outs {
        for &input in &ins {
            for mode in [
                TransitionMode::BeatBlend,
                TransitionMode::LoopRoll,
                TransitionMode::PhraseBridge,
            ] {
                let lengths: &[u16] = if mode == TransitionMode::BeatBlend {
                    &[16, 32, 8]
                } else if mode == TransitionMode::LoopRoll {
                    &[8, 16]
                } else {
                    &[4]
                };
                for &length in lengths {
                    if let Some(p) = pair(ctx, options, out, input, mode, length) {
                        if best.as_ref().is_none_or(|old| {
                            p.summary.as_ref().unwrap().score > old.summary.as_ref().unwrap().score
                        }) {
                            best = Some(p);
                        }
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
    length: u16,
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
    let loop_roll = mode == TransitionMode::LoopRoll;
    if options.strategy == Some(S::LoopConstruct) && !loop_roll {
        return None;
    }
    if options.strategy == Some(S::ScratchCut) && (blend || loop_roll) {
        return None;
    }
    let hold = options.strategy == Some(S::EnergyHold) && blend;
    let correction = outgoing_bpm * factor / incoming_bpm;
    let harmonic = shift == 0. && key >= 0.85 && a.key_confidence >= 0.5 && b.key_confidence >= 0.5;
    if blend && ((correction - 1.).abs() > 0.08 || ga.meter() != gb.meter()) {
        return None;
    }
    if loop_roll && ((correction - 1.).abs() > 0.12 || ga.meter() != gb.meter()) {
        return None;
    }
    let mut n = length;
    if let Some((min, _)) = user_range(ctx.cues_out, a.sample_rate) {
        n = n.max(((out - ga.floor_bar(min)) / ga.meter()).ceil().max(1.) as u16);
    }
    if blend || loop_roll {
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
    if !blend && !loop_roll {
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
    let out_phrase = boundary_quality(a, end);
    let in_phrase = boundary_quality(b, bin);
    let start_phrase = boundary_quality(a, start);
    let explicit_out = user_range(ctx.cues_out, a.sample_rate).is_some();
    let explicit_in = user_range(ctx.cues_in, b.sample_rate).is_some();
    // A metrical downbeat is not automatically the beginning of a phrase.
    // Strong structural evidence takes precedence over the old fixed offsets.
    if (!a.phrase_boundaries.is_empty() && !explicit_out && out_phrase < 0.5)
        || (!b.phrase_boundaries.is_empty() && !explicit_in && in_phrase < 0.5)
        || (blend && !a.phrase_boundaries.is_empty() && !explicit_out && start_phrase < 0.5)
    {
        return None;
    }
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
    let mut b_end = if blend {
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
    if loop_roll {
        let incoming = feature(b, bin + 0.01);
        let outgoing = feature(a, end - 0.01);
        if !grid_reliable(a, start, end)
            || !grid_reliable(b, bin, (bin + 8. * 60. / incoming_bpm).min(b.duration_sec))
            || !incoming.is_some_and(|f| {
                f.rms > 0.001
                    && f.kick_salience >= 0.45
                    && f.vocal_presence < 0.35
                    && f.vocal_confidence.is_none_or(|v| v < 0.35)
            })
            || !outgoing.is_some_and(|f| f.vocal_presence < 0.5)
        {
            return None;
        }
        let bars = if factor == 1. { 1 } else { 2 };
        let loop_length = gb.sec(input + bars as f32 * gb.meter()) - bin;
        let off_bar = (n * 0.625).max(4.0).min(n - 1.0);
        lanes.loop_b = Some(mixless_protocol::LoopOp {
            start_src_frame: (bin * b.sample_rate as f32).round() as u64,
            length_bars: bars,
            on_bar: 0.0,
            off_bar,
            length_src_frames: (loop_length * b.sample_rate as f32).round() as u64,
        });
        let exit_beat =
            input + (off_bar * ga.meter() * factor).rem_euclid(bars as f32 * gb.meter());
        b_end = gb.sec(exit_beat + (n - off_bar) * ga.meter() * factor);
        lanes.filter_a.lp_hz = line(&[
            (0., 20000.),
            ((off_bar - 2.).max(0.), 20000.),
            ((off_bar - 1.).max(0.), 1200.),
            (off_bar, 500.),
        ]);
        lanes.fx_send_a = line(&[
            (0., 0.),
            ((off_bar - 1.).max(0.), 0.),
            ((off_bar - 0.5).max(0.), 0.22),
            (off_bar, 0.),
        ]);
        lanes.gain_a = line(&[
            (0., 0.),
            ((off_bar - 0.5).max(0.), 0.),
            (off_bar, KILL),
            (n, KILL),
        ]);
        lanes.xfader = line(&[
            (0., -1.),
            ((off_bar - 0.5).max(0.), -1.),
            (off_bar, 1.),
            (n, 1.),
        ]);
        if b_end > b.duration_sec + 0.001
            || !covers_user_range(bin, b_end, ctx.cues_in, b.sample_rate)
        {
            return None;
        }
        if !harmonic && !percussion_only(a, start, end) && !percussion_only(b, bin, b_end) {
            return None;
        }
    }
    if blend && (!grid_reliable(a, start, end) || !grid_reliable(b, bin, b_end)) {
        return None;
    }
    if blend && !harmonic && !percussion_only(a, start, end) && !percussion_only(b, bin, b_end) {
        return None;
    }
    // Do not let a long intro blend consume the start of an incoming vocal hook
    // merely because the outgoing side is instrumental. End it on a phrase.
    let end_phrase = boundary_quality(b, b_end);
    if blend && !b.phrase_boundaries.is_empty() && !explicit_in && end_phrase < 0.5 {
        return None;
    }
    let mut clash = 0.;
    let mut vocal_a = 0.;
    let mut vocal_b = 0.;
    let mut clash_run = 0.;
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
            if matches!(
                (section(a, ta), section(b, tb)),
                (
                    mixless_protocol::SectionLabel::Verse,
                    mixless_protocol::SectionLabel::Chorus
                ) | (
                    mixless_protocol::SectionLabel::Chorus,
                    mixless_protocol::SectionLabel::Verse
                )
            ) {
                return None;
            }
            clash += 1. / samples as f32;
            clash_run += ga.meter() * n / samples as f32;
            // A long overlap must not dilute a whole colliding vocal line to
            // an acceptable percentage. Allow at most one beat of spillover.
            if blend && clash_run > 1. {
                return None;
            }
        } else {
            clash_run = 0.;
        }
    }
    if blend && clash > 0.0625 {
        return None;
    }
    let sa = section(a, end - 0.001);
    let sb = section(b, bin + 0.001);
    let drop_cut = !blend
        && matches!(sa, mixless_protocol::SectionLabel::BuildUp)
        && matches!(
            sb,
            mixless_protocol::SectionLabel::Drop | mixless_protocol::SectionLabel::Chorus
        )
        && out_phrase >= 0.6
        && in_phrase >= 0.6
        && feature(b, bin + 0.01).is_some_and(|f| f.kick_salience >= 0.5);
    // Scratch cuts are rare, cueable accents. Require reliable grids,
    // phrase edges, strong kicks and a user hot cue at the incoming hit.
    let scratch_cut = !blend
        && !loop_roll
        && out_phrase >= 0.75
        && in_phrase >= 0.75
        && grid_reliable(a, start, end)
        && grid_reliable(b, bin, (bin + 4. * 60. / incoming_bpm).min(b.duration_sec))
        && feature(a, end - 0.01)
            .is_some_and(|f| f.kick_salience >= 0.55 && f.vocal_presence < 0.35)
        && feature(b, bin + 0.01)
            .is_some_and(|f| f.kick_salience >= 0.55 && f.vocal_presence < 0.65)
        && ctx.cues_in.iter().any(|cue| {
            cue.user_set
                && matches!(cue.kind, mixless_protocol::CueKind::Hot)
                && (cue.frame as f32 / b.sample_rate as f32 - bin).abs() <= 60. / incoming_bpm
        })
        && (options.strategy == Some(S::ScratchCut) || drop_cut || out_phrase >= 0.9);
    let fx_decision = if blend {
        policy::Decision {
            technique: Technique::DryCut,
            confidence: 1.0,
        }
    } else {
        policy::choose(FxEvidence {
            loop_roll,
            scratch: scratch_cut,
            drop: drop_cut,
            grid_reliable: grid_reliable(a, start, end)
                && grid_reliable(b, bin, (bin + 4. * 60. / incoming_bpm).min(b.duration_sec)),
            phrase_reliable: out_phrase >= 0.6 && in_phrase >= 0.6,
            harmonic_compatible: harmonic,
            vocal_overlap: clash,
            outgoing_kick: feature(a, end - 0.01).map_or(0.0, |f| f.kick_salience),
            incoming_kick: feature(b, bin + 0.01).map_or(0.0, |f| f.kick_salience),
            outgoing_vocal: vocal_a,
            incoming_vocal: vocal_b,
            outgoing_section: sa,
        })
    };
    if (loop_roll && fx_decision.technique != Technique::LoopRoll)
        || (options.strategy == Some(S::ScratchCut)
            && fx_decision.technique != Technique::ScratchCut)
    {
        return None;
    }
    let handoff = if blend {
        bass_handoff(a, b, &source_a, &source_b, n)?
    } else {
        start_b
    };
    let rms_a = if blend {
        average_rms(
            a,
            source_a.sample((handoff - 2.).max(0.)),
            source_a.sample((handoff + 2.).min(n)),
        )
    } else {
        average_rms(
            a,
            (end - 4. * ga.meter() * 60. / outgoing_bpm).max(start),
            end,
        )
    };
    let rms_b = if blend {
        average_rms(
            b,
            source_b.sample((handoff - 2.).max(0.)),
            source_b.sample((handoff + 2.).min(n)),
        )
    } else {
        average_rms(
            b,
            bin,
            (bin + 4. * gb.meter() * 60. / incoming_bpm).min(b.duration_sec),
        )
    };
    // Match the material heard at the exchange, preserving source dynamics.
    let trim_b = incoming_trim(b, bin, rms_a, rms_b);
    let rhythmic_handoff = blend
        && (feature(a, source_a.sample(handoff) - 0.01).is_some_and(|f| f.kick_salience >= 0.5)
            || feature(b, source_b.sample(handoff) + 0.01).is_some_and(|f| f.kick_salience >= 0.5));
    if blend {
        let middle = handoff;
        let width = if rhythmic_handoff {
            0.5 / ga.meter()
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
        if matches!(
            fx_decision.technique,
            Technique::EchoOut | Technique::FilterBridge
        ) {
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
        if matches!(
            fx_decision.technique,
            Technique::DryCut | Technique::DropCut | Technique::ScratchCut
        ) {
            // A completed build can hand directly to the new drop. Keep its
            // buildup intact and avoid an FX tail masking the incoming first kick.
            lanes.filter_a = neutral().filter_a;
            lanes.fx_send_a = Polyline::constant(0.);
            lanes.eq_a.low = line(&[(0., 0.), (handoff - fade, 0.), (handoff, KILL)]);
        }
    }
    if scratch_cut {
        let on_bar = (n - 0.5).max(0.0);
        let peak_bar = (n - 0.25).max(on_bar + 0.0625);
        let start_src_frame = (source_a.sample(on_bar) * a.sample_rate as f32)
            .round()
            .max(0.0) as u64;
        let delta = (0.5 * a.sample_rate as f32 * 60. / outgoing_bpm).round() as i64;
        lanes.scratch_a = Some(ScratchOp {
            start_src_frame,
            peak_delta_frames: -delta,
            on_bar,
            peak_bar,
            off_bar: n,
        });
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
    // Phrase-compatible pairings are preferred even when BPM/key are equal.
    // The vocal rule is hard: a verse and a chorus with two foregrounds never
    // share the same overlap window.
    if sa == mixless_protocol::SectionLabel::Verse
        && sb == mixless_protocol::SectionLabel::Chorus
        && vocal_a > 0.45
        && vocal_b > 0.45
    {
        return None;
    }
    let structural_score = match (sa, sb) {
        (mixless_protocol::SectionLabel::Outro, mixless_protocol::SectionLabel::Intro)
        | (mixless_protocol::SectionLabel::Drop, mixless_protocol::SectionLabel::Intro)
        | (mixless_protocol::SectionLabel::BuildUp, mixless_protocol::SectionLabel::Drop)
        | (mixless_protocol::SectionLabel::Break, mixless_protocol::SectionLabel::Intro) => 1.0,
        (mixless_protocol::SectionLabel::Chorus, mixless_protocol::SectionLabel::Intro)
        | (mixless_protocol::SectionLabel::Breakdown, mixless_protocol::SectionLabel::Intro) => {
            0.85
        }
        (mixless_protocol::SectionLabel::Unknown, _)
        | (_, mixless_protocol::SectionLabel::Unknown) => 0.55,
        _ => 0.35,
    };
    let quality = if blend {
        0.57 + 0.10 * key + 0.10 * structural_score - 0.10 * clash
    } else if loop_roll {
        0.39 + 0.08 * structural_score + 0.05 * (1. - vocal_a.min(vocal_b))
    } else {
        0.35 + 0.10 * structural_score
            + 0.02 * fx_decision.confidence
            + 0.08 * (1. - vocal_a.min(vocal_b))
            + if drop_cut { 0.22 } else { 0. }
    };
    let phrase = if blend {
        (out_phrase + in_phrase + start_phrase + end_phrase) / 4.
    } else {
        (out_phrase + in_phrase) / 2.
    };
    let energy = match (rms_a, rms_b) {
        (Some(a), Some(b)) => (1. - (20. * (a / b).log10()).abs() / 18.).clamp(0., 1.),
        _ => 0.5,
    };
    let kick = match (feature(a, end - 0.01), feature(b, bin + 0.01)) {
        (Some(a), Some(b)) => 1. - (a.kick_salience - b.kick_salience).abs(),
        _ => 0.5,
    };
    let length_cost = if blend {
        (n - 16.).abs() / 16. * 0.008
    } else {
        0.
    };
    Some(MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: if blend {
                if hold {
                    S::EnergyHold
                } else if !harmonic || !rhythmic_handoff {
                    S::PhraseBlend
                } else {
                    S::BassSwap
                }
            } else if loop_roll {
                S::LoopConstruct
            } else {
                match fx_decision.technique {
                    Technique::DryCut => S::DryCut,
                    Technique::ScratchCut => S::ScratchCut,
                    Technique::DropCut => S::DropCut,
                    Technique::FilterBridge => S::FilterSweep,
                    _ => S::EchoOut,
                }
            },
            score: (quality + position + phrase * 0.12 + energy * 0.05 + kick * 0.03 - length_cost
                + if structural { 0.03 } else { 0. })
            .clamp(0., 1.),
            used_fallback: !blend
                && !loop_roll
                && !matches!(
                    fx_decision.technique,
                    Technique::DropCut | Technique::ScratchCut | Technique::DryCut
                ),
            length_bars: n as u16,
        }),
        incoming_offset_end: ending,
        outgoing_offset: ctx.offset_a,
        bar_map: if blend || loop_roll {
            map
        } else {
            BarMap::OneToOne
        },
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
        handoff_bar: Some(if loop_roll { n } else { handoff }),
        literal_half_double: false,
        failure_reason: None,
    })
}
