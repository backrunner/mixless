//! Conservative DJ policy: musical safety gates precede ranking. A scalar
//! score cannot compensate for clashing foregrounds or an unreliable beat grid.
use crate::{
    constraints::{covers_user_range, user_range},
    grid::Grid,
    musical::{average_rms, bass_handoff, feature, percussion_only},
    phrasing::{boundary_quality, points},
    policy::{self, Evidence as FxEvidence, Technique},
    score::{section, window_key_match},
    PlanContext, PlannerOptions,
};
use mixless_protocol::{
    AutomationLanes, BarMap, EqLane, FilterLane, MixPlan, MixPlanSummary, Polyline, ScratchOp,
    StrategyId as S, TrackAnalysis, TransitionMode,
};

mod candidate;
use candidate::pair;
mod lanes;
mod preparation;
mod ranking;
mod timing;
use lanes::{ease, neutral};

const MAX_LOG_TEMPO_PER_SEC: f32 = 0.0035;
const KILL: f32 = -96.;
fn fits_region(
    t: &TrackAnalysis,
    kind: mixless_protocol::MixRegionKind,
    anchor: f32,
    start: f32,
    end: f32,
) -> bool {
    let mut matching = t
        .mix_regions
        .iter()
        .filter(|r| r.kind == kind && (r.anchor_sec - anchor).abs() < 0.15)
        .peekable();
    matching.peek().is_none()
        || matching.any(|r| start + 0.05 >= r.start_sec && end <= r.end_sec + 0.05)
}
pub(super) fn grid_reliable(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    if t.tempo.beats.len() < 8 || t.tempo.downbeats.len() < 2 {
        return false;
    }
    let grid = Grid(t);
    let first = grid.beat(start).floor() as usize;
    let last = grid.beat(end).ceil() as usize;
    if last >= t.tempo.beats.len() {
        return false;
    }
    if first >= last {
        return false;
    }
    let segments = &t.tempo.segments;
    let confidence = |index: usize| {
        t.tempo
            .pulse_confidence
            .get(index)
            .copied()
            .unwrap_or(segments[index].confidence)
    };
    let mut errors = Vec::with_capacity(last - first);
    let mut checked_segment = None;
    for i in first..last {
        let Some(index) = segments
            .iter()
            .position(|s| i as f32 >= s.start_beat && (i as f32) < s.end_beat)
        else {
            return false;
        };
        if checked_segment != Some(index)
            && confidence(index) < 0.65
            && !(confidence(index) >= 0.35
                && crate::rhythm::supports_grid(
                    t,
                    grid.sec(segments[index].start_beat),
                    grid.sec(segments[index].end_beat),
                ))
        {
            // Continue a measured clock across a sparse break only when strong
            // anchors on BOTH sides agree. A dense off-grid passage cannot borrow
            // confidence, and a quiet tail with no right anchor remains uncertain.
            let left = (0..index).rev().find(|j| confidence(*j) >= 0.65);
            let right = (index + 1..segments.len()).find(|j| confidence(*j) >= 0.65);
            let (Some(left), Some(right)) = (left, right) else {
                return false;
            };
            let lo = grid.sec(segments[left].end_beat);
            let hi = grid.sec(segments[right].start_beat);
            let drift = (segments[left].bpm / segments[right].bpm - 1.).abs();
            if drift > 0.005
                || drift * (hi - lo) > 0.02
                || t.bars
                    .iter()
                    .filter(|b| b.start_sec < hi && b.end_sec > lo)
                    .any(|b| b.kick_salience >= 0.45)
            {
                return false;
            }
        }
        checked_segment = Some(index);
        let expected = 60. / grid.bpm(i as f32);
        errors.push(((t.tempo.beats[i + 1] - t.tempo.beats[i]) / expected - 1.).abs());
    }
    errors.sort_by(f32::total_cmp);
    // Real onset grids contain local jitter. A single 3% interval must not
    // reject an otherwise stable 32-bar blend and force a 40 ms cut.
    errors[errors.len() / 2] <= 0.035
        && errors[(errors.len() - 1) * 95 / 100] <= 0.10
        && *errors.last().unwrap() <= 0.20
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
                for length in crate::duration::candidates(ctx, options, out, input, mode) {
                    if let Some(p) = pair(
                        ctx,
                        options,
                        out,
                        input,
                        mode,
                        length,
                        best.as_ref()
                            .map_or(-1., |p| p.summary.as_ref().unwrap().score),
                    ) {
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

#[cfg(test)]
mod recovery_tests;
