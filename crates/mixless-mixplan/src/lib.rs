//! Pure next-pair planning. No decoding, devices or I/O; optional bounded one-track lookahead.
pub mod cue_policy;
mod handoff;
mod instant;
mod progression;
#[cfg(test)]
mod stem_regressions;
pub use handoff::short_handoff;
pub use performance::{performance_moves, LiveMoves, PerformanceLane, PerformanceMove};
mod arrangement;
mod bridge;
mod candidates;
mod choreography;
mod compile;
pub mod constraints;
mod continuity;
mod drops;
mod duration;
mod filtered;
mod grid;
mod lookahead;
mod musical;
pub mod performance;
mod phrasing;
mod policy;
mod recovery;
mod rhythm;
pub mod score;
mod smooth;
mod spectrum;
mod stem_mix;
mod vocals;

use constraints::{covers_user_range, user_range};
use grid::Grid;
use mixless_protocol::{
    BarMap, Cue, MixPlan, PerformanceOffset, StrategyId, TrackAnalysis, WhoStretches,
};

pub const SCORE_THRESHOLD: f32 = 0.35;
// Bass swap wins ties for stable kicks, as in the documented 128/126 fixture.
pub const STRATEGIES: [StrategyId; 9] = [
    StrategyId::BassSwap,
    StrategyId::PhraseBlend,
    StrategyId::DropCut,
    StrategyId::EchoOut,
    StrategyId::FilterSweep,
    StrategyId::LoopConstruct,
    StrategyId::ScratchCut,
    StrategyId::BreakToIntro,
    StrategyId::EnergyHold,
];

#[derive(Debug, Clone)]
pub struct PlannerOptions {
    /// Set the inaudible incoming deck by at most two semitones, then hold it.
    pub harmonic_key_shift: bool,
    /// Audition-oriented policy. False preserves the original catalog golden tables.
    pub smooth: bool,
    pub who_stretches: WhoStretches,
    pub literal_half_double: bool,
    /// Optional catalog selection; hard constraints still apply.
    pub strategy: Option<StrategyId>,
    /// Do not schedule a transition behind the audible outgoing playhead.
    pub earliest_outgoing_sec: f32,
    /// Audible entry on this pass, including time spent in the incoming blend.
    pub outgoing_entry_sec: f32,
    /// Both decks will play aligned stem buffers, enabling drum-layered blends
    /// where tonal material would otherwise clash.
    pub stem_playback: bool,
}
impl Default for PlannerOptions {
    fn default() -> Self {
        Self {
            harmonic_key_shift: false,
            smooth: true,
            who_stretches: WhoStretches::B,
            literal_half_double: false,
            strategy: None,
            earliest_outgoing_sec: 0.0,
            outgoing_entry_sec: 0.0,
            stem_playback: false,
        }
    }
}
#[derive(Default)]
pub struct Planner {
    pub options: PlannerOptions,
}

pub struct PlanContext<'a> {
    pub outgoing: &'a TrackAnalysis,
    pub incoming: &'a TrackAnalysis,
    pub cues_out: &'a [Cue],
    pub cues_in: &'a [Cue],
    pub offset_a: PerformanceOffset,
    pub offset_b: PerformanceOffset,
}

#[derive(Clone)]
pub(crate) struct Candidate {
    strategy: StrategyId,
    n: u16,
    start_sec: f32,
    out_sec: f32,
    in_sec: f32,
    end_sec: f32,
    out_beat: f32,
    in_beat: f32,
    start_beat: f32,
    beat_factor: f32,
    bar_map: BarMap,
    ratio: f32,
    rate_a: f32,
    rate_b: f32,
    key_score: f32,
    shift_b: f32,
    offset_a: PerformanceOffset,
    offset_b: PerformanceOffset,
    literal: bool,
}

impl Planner {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_options(options: PlannerOptions) -> Self {
        Self { options }
    }

    pub fn plan_pair(
        &self,
        a: &TrackAnalysis,
        b: &TrackAnalysis,
        cues_a: &[Cue],
        cues_b: &[Cue],
        offset_a: PerformanceOffset,
        offset_b: PerformanceOffset,
    ) -> MixPlan {
        self.plan_next(&PlanContext {
            outgoing: a,
            incoming: b,
            cues_out: cues_a,
            cues_in: cues_b,
            offset_a,
            offset_b,
        })
    }

    pub fn plan_next(&self, ctx: &PlanContext<'_>) -> MixPlan {
        let mut plan = self.plan_next_base(ctx);
        stem_mix::arrange(ctx, &mut plan);
        plan
    }

    fn plan_next_base(&self, ctx: &PlanContext<'_>) -> MixPlan {
        let fail = |message: &str| MixPlan {
            failure_reason: Some(message.into()),
            ..MixPlan::default()
        };
        let (a, b) = (ctx.outgoing, ctx.incoming);
        for track in [a, b] {
            if track
                .stems
                .as_ref()
                .is_some_and(|s| !s.valid(track.duration_sec))
            {
                return fail("Invalid or incomplete stem evidence");
            }
            if track.moments.iter().any(|m| {
                ![
                    m.start_sec,
                    m.end_sec,
                    m.rms,
                    m.onset,
                    m.sustain,
                    m.attack_sec,
                    m.low_onset,
                ]
                .iter()
                .all(|v| v.is_finite())
                    || m.start_sec < 0.
                    || m.end_sec <= m.start_sec
                    || m.end_sec > track.duration_sec + 0.1
                    || m.rms < 0.
                    || ![m.onset, m.sustain, m.low_onset]
                        .iter()
                        .all(|v| (0. ..=1.).contains(v))
                    || m.band_db
                        .iter()
                        .chain(m.chroma.iter())
                        .any(|v| !v.is_finite())
                    || m.chroma.iter().any(|v| *v < 0.)
                    || m.pitch_midi.is_some_and(|v| !v.is_finite())
                    || m.vocal_confidence
                        .is_some_and(|v| !v.is_finite() || !(0. ..=1.).contains(&v))
            }) || track
                .moments
                .windows(2)
                .any(|p| p[0].end_sec > p[1].start_sec + 0.001)
            {
                return fail("Invalid spectral observations");
            }

            if track.bars.iter().any(|b| {
                !b.start_sec.is_finite()
                    || !b.end_sec.is_finite()
                    || b.start_sec < 0.
                    || b.end_sec <= b.start_sec
                    || b.end_sec > track.duration_sec + 0.1
                    || !b.rms.is_finite()
                    || b.rms < 0.
                    || !b.crest.is_finite()
                    || b.crest < 0.
                    || !b.onset_density.is_finite()
                    || b.onset_density < 0.
                    || [b.low_db, b.mid_db, b.high_db, b.energy_slope]
                        .iter()
                        .any(|v| !v.is_finite())
                    || [b.vocal_presence, b.kick_salience, b.hat_salience]
                        .iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                    || b.vocal_confidence
                        .is_some_and(|v| !v.is_finite() || !(0.0..=1.0).contains(&v))
                    || b.chroma.iter().any(|v| !v.is_finite() || *v < 0.)
            }) || track
                .bars
                .windows(2)
                .any(|p| p[1].start_sec < p[0].end_sec - 0.001)
            {
                return fail("Invalid or overlapping bar features");
            }
            if track.sections.iter().any(|s| {
                !s.start_sec.is_finite()
                    || !s.end_sec.is_finite()
                    || s.start_sec < 0.
                    || s.end_sec <= s.start_sec
                    || s.end_sec > track.duration_sec + 0.1
            }) {
                return fail("Invalid section bounds");
            }
            if track.mix_regions.iter().any(|r| {
                ![
                    r.start_sec,
                    r.end_sec,
                    r.anchor_sec,
                    r.confidence,
                    r.vocal_risk,
                    r.kick,
                    r.rms,
                    r.key_confidence,
                ]
                .iter()
                .all(|v| v.is_finite())
                    || r.start_sec < 0.
                    || r.end_sec <= r.start_sec
                    || r.end_sec > track.duration_sec + 0.1
                    || r.anchor_sec < r.start_sec
                    || r.anchor_sec > r.end_sec
                    || ![r.confidence, r.vocal_risk, r.kick, r.key_confidence]
                        .iter()
                        .all(|v| (0.0..=1.0).contains(v))
                    || r.rms < 0.
            }) {
                return fail("Invalid mix region evidence");
            }
            if !track.duration_sec.is_finite()
                || track.duration_sec <= 0.0
                || track.sample_rate == 0
                || track.tempo.meter_num == 0
                || track.tempo.beats.iter().any(|t| !t.is_finite() || *t < 0.0)
                || track.tempo.beats.windows(2).any(|p| p[1] <= p[0])
                || track
                    .tempo
                    .downbeats
                    .iter()
                    .any(|t| !t.is_finite() || *t < 0.0)
                || track.tempo.downbeats.windows(2).any(|p| p[1] <= p[0])
                || track.phrase_boundaries.iter().any(|p| {
                    !p.time_sec.is_finite()
                        || p.time_sec < 0.
                        || p.time_sec > track.duration_sec + 0.1
                        || !p.confidence.is_finite()
                        || !(0.0..=1.0).contains(&p.confidence)
                        || !p.novelty.is_finite()
                        || p.novelty < 0.
                })
            {
                return fail("Invalid duration, source sample rate or beat grid");
            }
        }
        for offset in [ctx.offset_a, ctx.offset_b] {
            if !offset.rate.is_finite()
                || !(0.5..=2.0).contains(&offset.rate)
                || !offset.pitch_semitones.is_finite()
                || offset.pitch_semitones.abs() > 24.0
            {
                return fail("Invalid performance offset");
            }
        }
        if !self.options.earliest_outgoing_sec.is_finite() {
            return fail("Invalid outgoing playhead");
        }
        if self.options.smooth {
            return smooth::plan(ctx, &self.options);
        }
        let outs = candidates::candidates(a, ctx.cues_out, true);
        let ins = candidates::candidates(b, ctx.cues_in, false);
        let mut best: Option<(f32, Candidate)> = None;
        for &o in &outs {
            for &i in &ins {
                for strategy in STRATEGIES {
                    if self.options.strategy.is_some_and(|s| s != strategy) {
                        continue;
                    }
                    if let Some(c) = self.candidate(ctx, o, i, strategy, false) {
                        if let Some(sc) = score::score(&c, a, b, ctx.cues_out, ctx.cues_in) {
                            if best
                                .as_ref()
                                .is_none_or(|(previous, _)| sc > *previous + 0.00001)
                            {
                                best = Some((sc, c));
                            }
                        }
                    }
                }
            }
        }
        if let Some((sc, c)) = best {
            if sc >= SCORE_THRESHOLD {
                return compile::compile(&c, a, b, sc, false);
            }
        }
        // Fallback is constrained too. Prefer the first incoming downbeat and
        // the final outgoing high-energy section; shorten only when necessary.
        let mut fallback_ins = ins;
        fallback_ins.sort_by(f32::total_cmp);
        for &o in &outs {
            for &i in &fallback_ins {
                if let Some(c) = self.candidate(ctx, o, i, StrategyId::FallbackSwapFilter, true) {
                    return compile::compile(&c, a, b, 0.0, true);
                }
            }
        }
        fail(
            "No safe remaining window satisfies track bounds, user mix-in/out cues and vocal exclusions",
        )
    }

    fn candidate(
        &self,
        ctx: &PlanContext<'_>,
        out_beat: f32,
        in_beat: f32,
        strategy: StrategyId,
        fallback: bool,
    ) -> Option<Candidate> {
        let (a, b) = (ctx.outgoing, ctx.incoming);
        let ga = Grid(a);
        let gb = Grid(b);
        let bpm_a = ga.bpm((out_beat - 0.01).max(0.0)) * ctx.offset_a.rate;
        let bpm_b = gb.bpm(in_beat) * ctx.offset_b.rate;
        let ratio = bpm_a / bpm_b;
        let double = (ratio / 2.0 - 1.0).abs() < 0.08;
        let half = (ratio / 0.5 - 1.0).abs() < 0.08;
        let literal = self.options.literal_half_double && (double || half);
        let (bar_map, beat_factor) = if !literal && (double || half) {
            (
                BarMap::TwoToOne {
                    outgoing_is_double: double,
                },
                if double { 0.5 } else { 2.0 },
            )
        } else {
            (BarMap::OneToOne, 1.0)
        };
        let correction = ratio * beat_factor;
        let who = if strategy == StrategyId::EnergyHold {
            WhoStretches::B
        } else {
            self.options.who_stretches
        };
        let (rate_a, rate_b) = match who {
            WhoStretches::A => (ctx.offset_a.rate / correction, ctx.offset_b.rate),
            WhoStretches::B => (ctx.offset_a.rate, ctx.offset_b.rate * correction),
            WhoStretches::Both => (
                ctx.offset_a.rate / correction.sqrt(),
                ctx.offset_b.rate * correction.sqrt(),
            ),
        };
        if !(0.5..=2.0).contains(&rate_a) || !(0.5..=2.0).contains(&rate_b) {
            return None;
        }
        let out_sec = ga.sec(out_beat);
        let in_sec = gb.sec(in_beat);
        let mut n = strategy.default_bars();
        let mut required = 1;
        if let Some((min, _)) = user_range(ctx.cues_out, a.sample_rate) {
            required = required.max(
                ((out_beat - ga.floor_bar(min)) / ga.meter())
                    .ceil()
                    .max(1.0) as u16,
            );
        }
        if let Some((_, max)) = user_range(ctx.cues_in, b.sample_rate) {
            required = required.max(
                ((gb.ceil_bar(max) - in_beat) / (ga.meter() * beat_factor))
                    .ceil()
                    .max(1.0) as u16,
            );
        }
        n = n.max(required);
        let earliest = ga.ceil_bar(self.options.earliest_outgoing_sec.max(0.0));
        let available_a = ((out_beat - earliest) / ga.meter() + 0.00001)
            .floor()
            .max(0.0) as u16;
        let available_b = ((gb.beat(b.duration_sec) - in_beat) / (ga.meter() * beat_factor)
            + 0.00001)
            .floor()
            .max(0.0) as u16;
        if fallback {
            n = n.min(available_a).min(available_b);
        }
        if n == 0 || n < required || n > available_a || n > available_b {
            return None;
        }
        let start_beat = out_beat - n as f32 * ga.meter();
        let start_sec = ga.sec(start_beat);
        let end_sec = gb.sec(in_beat + n as f32 * ga.meter() * beat_factor);
        if start_sec < 0.0
            || out_sec > a.duration_sec + 0.001
            || end_sec > b.duration_sec + 0.001
            || !covers_user_range(start_sec, out_sec, ctx.cues_out, a.sample_rate)
            || !covers_user_range(in_sec, end_sec, ctx.cues_in, b.sample_rate)
        {
            return None;
        }
        // Loop content is not a continuous source range; don't claim to cover
        // explicit incoming cues beyond its actual exit playhead.
        if strategy == StrategyId::LoopConstruct && user_range(ctx.cues_in, b.sample_rate).is_some()
        {
            return None;
        }
        let (key_score, shift_b) = score::key_match(
            a,
            b,
            ctx.offset_a.pitch_semitones,
            ctx.offset_b.pitch_semitones,
        );
        let candidate = Candidate {
            strategy,
            n,
            start_sec,
            out_sec,
            in_sec,
            end_sec,
            out_beat,
            in_beat,
            start_beat,
            beat_factor,
            bar_map,
            ratio,
            rate_a,
            rate_b,
            key_score,
            shift_b,
            offset_a: ctx.offset_a,
            offset_b: ctx.offset_b,
            literal,
        };
        // Musical hard exclusions apply to the fallback as well.
        (!score::vocal_forbidden(&candidate, a, b)).then_some(candidate)
    }
}

#[cfg(test)]
mod tests;
