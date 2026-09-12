//! Integrate source clocks and tempo curves before shaping audible envelopes.
use super::*;

pub(super) struct Timing {
    pub lanes: AutomationLanes,
    pub clock: Polyline,
    pub source_a: Polyline,
    pub source_b: Polyline,
    pub master: Polyline,
    pub b_end: f32,
    pub start_b: f32,
}

pub(super) fn prepare(
    ctx: &PlanContext<'_>,
    mode: TransitionMode,
    n: f32,
    start_beat: f32,
    input: f32,
    bin: f32,
    factor: f32,
    hold: bool,
    key_shift: f32,
    harmonic: bool,
    explicit_in: bool,
) -> Option<Timing> {
    let (a, b) = (ctx.outgoing, ctx.incoming);
    let (ga, gb) = (Grid(a), Grid(b));
    let start = ga.sec(start_beat);
    let end = ga.sec(start_beat + n * ga.meter());
    let incoming_bpm = gb.bpm(input) * ctx.offset_b.rate;
    let bpm_start = ga.bpm(start_beat) * ctx.offset_a.rate;
    let blend = mode == TransitionMode::BeatBlend;
    let loop_roll = mode == TransitionMode::LoopRoll;
    let mut lanes = neutral();
    lanes.rate_a.nodes.clear();
    lanes.rate_b.nodes.clear();
    lanes.pitch_a = Polyline::constant(ctx.offset_a.pitch_semitones);
    lanes.pitch_b = Polyline::constant(ctx.offset_b.pitch_semitones + key_shift);
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
    let start_b = if blend || loop_roll { 0. } else { n };
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
    // Leave an incoming phrase to play after the overlap, rather than
    // consuming the entire next track in a long mix.
    if blend && !explicit_in {
        let reserve = (8. * gb.meter() * 60. / incoming_bpm).min(b.duration_sec * 0.25);
        if b_end > b.duration_sec - reserve + 0.01 {
            return None;
        }
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
        lanes.xfader.nodes = (0..=n as usize * 64)
            .map(|j| {
                let u = j as f32 / 64.;
                (u, -1. + 2. * ease(u / off_bar))
            })
            .collect();
        lanes.eq_b.low = line(&[(0., KILL), ((off_bar - 0.25).max(0.), KILL), (off_bar, 0.)]);
        lanes.eq_a.low = line(&[(0., 0.), ((off_bar - 0.25).max(0.), 0.), (off_bar, KILL)]);
        lanes.gain_b = Polyline::constant(0.);
        lanes.rate_b.nodes.clear();
        clock.nodes.retain(|(u, _)| *u <= n);
        for j in 0..=n as usize * 64 {
            let u = j as f32 / 64.;
            // The source map is unwrapped; the engine applies the loop window.
            let beat = input + u * ga.meter() * factor;
            let rate =
                ga.bpm(start_beat + u * ga.meter()) * ctx.offset_a.rate * factor / gb.bpm(beat);
            if !(0.88..=1.12).contains(&(rate / ctx.offset_b.rate)) {
                return None;
            }
            lanes.rate_b.nodes.push((u, rate));
            source_b.nodes.push((u, gb.sec(beat)));
        }
        if b_end > b.duration_sec + 0.001
            || !covers_user_range(bin, b_end, ctx.cues_in, b.sample_rate)
        {
            return None;
        }
        if !harmonic && !percussion_only(a, start, end) && !percussion_only(b, bin, b_end) {
            return None;
        }
    }
    Some(Timing {
        lanes,
        clock,
        source_a,
        source_b,
        master,
        b_end,
        start_b,
    })
}
