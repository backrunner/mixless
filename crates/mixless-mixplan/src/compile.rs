use crate::{Candidate, grid::Grid};
use mixless_protocol::{
    AutomationLanes, EqLane, FilterLane, LoopOp, MixPlan, MixPlanSummary, PerformanceOffset,
    Polyline, ScratchOp, StrategyId as S, TrackAnalysis,
};

pub const KILL: f32 = -96.0;
// u, xf, gain A/B, low A/B, mid A/B, high A/B, LP A, HP B, send A.
// Unlisted filters and send B are OPEN / zero in every catalog strategy.
type Row = [f32; 13];
fn rows(strategy: S) -> Vec<Row> {
    match strategy {
        S::PhraseBlend | S::EnergyHold => vec![
            [0., -1., 0., -3., 0., -6., 0., -3., 0., 0., 20000., 20., 0.],
            [0.5, 0., 0., 0., -3., 0., 0., 0., -3., 0., 20000., 20., 0.],
            [
                1., 1., KILL, 0., KILL, 0., -6., 0., KILL, 0., 20000., 20., 0.,
            ],
        ],
        S::BassSwap | S::BreakToIntro | S::FallbackSwapFilter => {
            let mut r = vec![
                [
                    0., -0.3, 0., -6., 0., KILL, 0., 0., 0., -3., 20000., 20., 0.,
                ],
                [0.5, 0.2, 0., 0., KILL, 0., 0., 0., -3., 0., 20000., 20., 0.],
                [
                    1., 1., KILL, 0., KILL, 0., -6., 0., KILL, 0., 20000., 20., 0.,
                ],
            ];
            if strategy == S::BreakToIntro {
                r[0][1] = -0.5;
            }
            if strategy == S::FallbackSwapFilter {
                for (row, (lp, hp)) in
                    r.iter_mut()
                        .zip([(20000., 2500.), (800., 400.), (200., 20.)])
                {
                    row[10] = lp;
                    row[11] = hp;
                }
            }
            r
        }
        S::DryCut => vec![
            [0., -1., 0., -6., 0., 0., 0., 0., 0., 0., 20000., 20., 0.],
            [0.75, -1., 0., 0., 0., 0., 0., 0., 0., 0., 20000., 20., 0.],
            [1., 1., KILL, 0., 0., 0., 0., 0., 0., 0., 20000., 20., 0.],
        ],
        S::ScratchCut => vec![
            [0., -1., 0., -6., 0., 0., 0., 0., 0., 0., 20000., 20., 0.],
            [0.75, -1., 0., 0., 0., 0., 0., 0., 0., 0., 20000., 20., 0.],
            [1., 1., KILL, 0., 0., 0., 0., 0., 0., 0., 20000., 20., 0.],
        ],
        S::DropCut => vec![
            [0., -1., 0., -12., 0., 0., 0., 0., 0., 0., 20000., 20., 0.],
            [0.75, -1., 0., 0., 0., 0., 0., 0., 0., 0., 20000., 20., 0.6],
            [
                0.8125, 0., -6., 0., 0., 0., 0., 0., 0., 0., 20000., 20., 0.8,
            ],
            [
                1., 1., KILL, 0., KILL, 0., KILL, 0., KILL, 0., 20000., 20., 0.2,
            ],
        ],
        S::EchoOut => vec![
            [0., -0.6, 0., -3., 0., 0., 0., 0., 0., 0., 20000., 20., 0.3],
            [
                0.25, -0.2, -3., 0., -6., 0., 0., 0., 0., 0., 20000., 20., 0.8,
            ],
            [
                1., 1., KILL, 0., KILL, 0., KILL, 0., KILL, 0., 20000., 20., 0.4,
            ],
        ],
        S::FilterSweep => vec![
            [0., -1., 0., -6., 0., 0., 0., 0., 0., 0., 20000., 2500., 0.],
            [0.5, 0., 0., 0., 0., 0., 0., 0., 0., 0., 800., 400., 0.],
            [1., 1., KILL, 0., 0., 0., 0., 0., 0., 0., 200., 20., 0.],
        ],
        S::LoopConstruct => vec![
            [
                0., -0.5, 0., -9., 0., KILL, 0., -3., 0., 0., 20000., 20., 0.,
            ],
            [0.5, 0.2, 0., 0., -6., 0., 0., 0., -3., 0., 20000., 20., 0.],
            [
                0.625, 0.4, -3., 0., KILL, 0., 0., 0., -3., 0., 20000., 20., 0.,
            ],
            [1., 1., KILL, 0., KILL, 0., 0., 0., -3., 0., 20000., 20., 0.],
        ],
    }
}

pub(crate) fn compile(
    c: &Candidate,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    score: f32,
    fallback: bool,
) -> MixPlan {
    let n = c.n as f32;
    let table = rows(c.strategy);
    let col = |index: usize| Polyline {
        nodes: table.iter().map(|r| (r[0] * n, r[index])).collect(),
    };
    let constant = Polyline::constant;
    let ga = Grid(a);
    let gb = Grid(b);
    let mut lanes = AutomationLanes {
        xfader: col(1),
        gain_a: col(2),
        gain_b: col(3),
        eq_a: EqLane {
            low: col(4),
            mid: col(6),
            high: col(8),
        },
        eq_b: EqLane {
            low: col(5),
            mid: col(7),
            high: col(9),
        },
        filter_a: FilterLane {
            lp_hz: col(10),
            hp_hz: constant(20.0),
        },
        filter_b: FilterLane {
            lp_hz: constant(20000.0),
            hp_hz: col(11),
        },
        fx_send_a: col(12),
        fx_send_b: constant(0.0),
        rate_a: constant(c.rate_a),
        rate_b: constant(c.rate_b),
        pitch_a: constant(c.offset_a.pitch_semitones),
        pitch_b: constant(c.offset_b.pitch_semitones + c.shift_b),
        loop_a: None,
        loop_b: None,
        scratch_a: None,
        scratch_b: None,
    };
    if c.strategy == S::PhraseBlend {
        lanes.pitch_b.nodes = vec![
            (0., c.offset_b.pitch_semitones + c.shift_b),
            (n * 0.5, (c.offset_b.pitch_semitones + c.shift_b) * 0.5),
            (n, 0.),
        ];
    }
    let mut clock = Polyline::default();
    let mut incoming_source = Polyline::default();
    lanes.rate_b.nodes.clear();
    // Sample the warp at quarter-bar boundaries. B source beats advance by the
    // map factor, while outgoing source time defines the wall clock.
    for j in 0..=c.n as usize * 4 {
        let u = j as f32 / 4.0;
        let beat_a = c.start_beat + u * ga.meter();
        let beat_b = c.in_beat + u * ga.meter() * c.beat_factor;
        clock
            .nodes
            .push((u, (ga.sec(beat_a) - c.start_sec) / c.rate_a));
        incoming_source.nodes.push((u, gb.sec(beat_b)));
        lanes.rate_b.nodes.push((
            u,
            ga.bpm(beat_a) * c.rate_a * c.beat_factor / gb.bpm(beat_b),
        ));
    }
    let mut t_end_b = c.end_sec;
    if c.strategy == S::LoopConstruct {
        let bars = if (c.ratio - 1.).abs() > 0.08 { 2 } else { 1 };
        let length = gb.sec(c.in_beat + bars as f32 * gb.meter()) - c.in_sec;
        lanes.loop_b = Some(LoopOp {
            start_src_frame: (c.in_sec * b.sample_rate as f32).round() as u64,
            length_bars: bars,
            on_bar: 0.,
            off_bar: n * 0.625,
            length_src_frames: (length * b.sample_rate as f32).round() as u64,
        });
        let exit_beat = c.in_beat
            + (n * 0.625 * ga.meter() * c.beat_factor).rem_euclid(bars as f32 * gb.meter());
        t_end_b = gb.sec(exit_beat + n * 0.375 * ga.meter() * c.beat_factor);
    }
    if c.strategy == S::ScratchCut {
        let on_bar = (n - 0.5).max(0.0);
        let peak_bar = (n - 0.25).max(on_bar + 0.0625);
        let start_beat = c.start_beat + on_bar * ga.meter();
        let start_src_frame = (ga.sec(start_beat) * a.sample_rate as f32).round().max(0.0) as u64;
        let bpm = ga.bpm(start_beat) * c.rate_a;
        let delta = (0.5 * a.sample_rate as f32 * 60. / bpm).round() as i64;
        lanes.scratch_a = Some(ScratchOp {
            start_src_frame,
            peak_delta_frames: -delta,
            on_bar,
            peak_bar,
            off_bar: n,
        });
    }
    let held_rate = lanes.rate_b.sample(n);
    let held_pitch = lanes.pitch_b.sample(n);
    let hold = c.strategy == S::EnergyHold;
    // Catalog rate nodes cover the overlap. Non-hold strategies recover only
    // once A is silent; this preserves the documented constant rB envelope.
    if !hold && ((held_rate - 1.0).abs() > 0.0001 || held_pitch.abs() > 0.0001) {
        let overlap_sec = clock.sample(n);
        let recovery_sec = (4.0 * ga.meter() * 60.0 / (ga.bpm(c.out_beat) * c.rate_a))
            .min(((b.duration_sec - t_end_b) / ((held_rate + 1.0) * 0.5)).max(0.0));
        if recovery_sec > 0.0 {
            let recovery_bars = recovery_sec * ga.bpm(c.out_beat) * c.rate_a / (60.0 * ga.meter());
            clock
                .nodes
                .push((n + recovery_bars, overlap_sec + recovery_sec));
            lanes.rate_b.nodes.push((n + recovery_bars, 1.0));
            if lanes.pitch_b.nodes.last().is_none_or(|p| p.0 < n) {
                lanes.pitch_b.nodes.push((n, held_pitch));
            }
            lanes.pitch_b.nodes.push((n + recovery_bars, 0.0));
        }
    }
    MixPlan {
        summary: Some(MixPlanSummary {
            pair: (a.track_id, b.track_id),
            strategy: c.strategy,
            score,
            used_fallback: fallback,
            length_bars: c.n,
        }),
        incoming_offset_end: if hold {
            PerformanceOffset {
                rate: held_rate,
                pitch_semitones: held_pitch,
            }
        } else {
            PerformanceOffset::identity()
        },
        outgoing_offset: c.offset_a,
        bar_map: c.bar_map,
        t_in_a: c.start_sec,
        t_out_a: c.out_sec,
        t_in_b: c.in_sec,
        t_end_b,
        clock,
        incoming_source,
        outgoing_source: Polyline::default(),
        incoming_start_bar: 0.0,
        transition_mode: None,
        stages: Vec::new(),
        master_bpm: Polyline::default(),
        lanes,
        handoff_bar: hold.then_some(n * 0.5),
        literal_half_double: c.literal,
        failure_reason: None,
    }
}
