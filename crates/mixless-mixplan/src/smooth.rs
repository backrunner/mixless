//! Conservative DJ policy: musical safety gates precede ranking. A scalar
//! score cannot compensate for clashing foregrounds or an unreliable beat grid.
use crate::{
    constraints::{covers_user_range, user_range},
    grid::Grid,
    musical::{average_rms, bass_handoff, feature, incoming_trim, percussion_only},
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
mod ranking;
mod timing;
use lanes::{ease, line, neutral};

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
    let mut errors = Vec::with_capacity(last - first);
    for i in first..last {
        if !t
            .tempo
            .segments
            .iter()
            .any(|s| i as f32 >= s.start_beat && (i as f32) < s.end_beat && s.confidence >= 0.65)
        {
            return false;
        }
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
