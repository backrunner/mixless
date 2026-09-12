//! Envelope compilation is host-only. The callback reads immutable 32-frame
//! samples and atomics; replaced buffers are reclaimed by the host.
use super::*;
use mixless_protocol::{LaneId, MixPlan, MixPlanSummary, Polyline};

const STEP: usize = 32;
const XF: u32 = 1;
fn bit(index: usize, lane: usize) -> u32 {
    1 << (1 + index * 7 + lane)
}
// per physical deck: gain, EQ, filter, send, rate, pitch, loop.

#[derive(Default)]
pub(super) struct AutomationShared {
    published: Mutex<Option<Arc<DensePlan>>>,
    retired: Mutex<Vec<Arc<DensePlan>>>,
    pub(super) enabled: AtomicBool,
    pub(super) paused: AtomicBool,
    pub(super) progress: AtomicU32,
    cancelled: AtomicU32,
    skip: AtomicBool,
}
#[derive(Default)]
pub(super) struct AutomationRt {
    plan: Option<Arc<DensePlan>>,
    elapsed: usize,
    started: bool,
    paused: bool,
    pub(super) clock_deck: Option<usize>,
    pub(super) tempo: Option<f32>,
    incoming_started: bool,
    resume_playing: [bool; 2],
}
#[derive(Clone, Copy)]
struct DeckPoint {
    gain: u32,
    fader: u32,
    eq: [u32; 3],
    filter: u32,
    send: u32,
    rate: u32,
    pitch: u32,
    loop_on: bool,
    scratch_touch: bool,
    scratch_target: u64,
}
#[derive(Clone, Copy)]
struct Point {
    xf: u32,
    decks: [DeckPoint; 2],
    tempo: f32,
}
pub struct PreparedMix(Arc<DensePlan>);

struct DensePlan {
    plan: MixPlan,
    outgoing: usize,
    start_frame: f64,
    incoming_frame: u64,
    incoming_start_frames: usize,
    overlap_frames: usize,
    handoff_frame: Option<usize>,
    total_frames: usize,
    sample_rate: u32,
    points: Vec<Point>,
}
fn db(db: f32) -> u32 {
    ((db.clamp(-96.0, 12.0) + 96.0) * 100.0).round() as u32
}
fn valid_line(line: &Polyline, min: f32, max: f32) -> bool {
    !line.nodes.is_empty()
        && line
            .nodes
            .iter()
            .all(|(u, v)| u.is_finite() && *u >= 0.0 && v.is_finite() && *v >= min && *v <= max)
        && line.nodes.windows(2).all(|p| p[1].0 > p[0].0)
}
fn source_at_seconds(plan: &MixPlan, source: &Polyline, sec: f64) -> f64 {
    let clock = &plan.clock.nodes;
    let j = clock
        .partition_point(|(_, t)| *t as f64 <= sec)
        .saturating_sub(1)
        .min(clock.len() - 2);
    let (a, x) = clock[j];
    let (b, y) = clock[j + 1];
    let u = a as f64 + (sec - x as f64) / (y as f64 - x as f64) * (b as f64 - a as f64);
    let source = &source.nodes;
    let j = source
        .partition_point(|(bar, _)| *bar as f64 <= u)
        .saturating_sub(1)
        .min(source.len() - 2);
    let (a, x) = source[j];
    let (b, y) = source[j + 1];
    x as f64 + (u - a as f64) / (b as f64 - a as f64) * (y as f64 - x as f64)
}
fn bar_at(clock: &Polyline, sec: f32) -> f32 {
    let j = clock
        .nodes
        .partition_point(|(_, t)| *t <= sec)
        .saturating_sub(1)
        .min(clock.nodes.len() - 2);
    let (a, x) = clock.nodes[j];
    let (b, y) = clock.nodes[j + 1];
    a + ((sec - x) / (y - x)).clamp(0.0, 1.0) * (b - a)
}
impl DensePlan {
    fn compile(plan: MixPlan, outgoing: usize, shared: &Shared) -> Result<Self, EngineError> {
        let invalid = || EngineError::Protocol("invalid automix envelope or timing");
        let summary = plan.summary.as_ref().ok_or_else(invalid)?;
        if plan.failure_reason.is_some()
            || summary.length_bars == 0
            || !summary.score.is_finite()
            || !(0.0..=1.0).contains(&summary.score)
            || !valid_line(&plan.clock, 0.0, 1800.0)
            || plan.clock.nodes.len() < 2
            || plan.clock.nodes[0] != (0.0, 0.0)
            || plan.clock.nodes.windows(2).any(|p| p[1].1 <= p[0].1)
            || ![plan.t_in_a, plan.t_out_a, plan.t_in_b, plan.t_end_b]
                .iter()
                .all(|s| s.is_finite() && *s >= 0.0)
            || plan.t_out_a <= plan.t_in_a
            || plan.t_end_b <= plan.t_in_b
        {
            return Err(invalid());
        }
        if !plan.incoming_source.nodes.is_empty()
            && (plan.incoming_source.nodes.len() < 2
                || !valid_line(&plan.incoming_source, 0.0, 86400.0)
                || plan
                    .incoming_source
                    .nodes
                    .windows(2)
                    .any(|p| p[1].1 <= p[0].1))
        {
            return Err(invalid());
        }
        for curve in [&plan.outgoing_source] {
            if !curve.nodes.is_empty()
                && (curve.nodes.len() < 2
                    || !valid_line(curve, 0., 86400.)
                    || curve.nodes.windows(2).any(|p| p[1].1 <= p[0].1))
            {
                return Err(invalid());
            }
        }
        if !plan.incoming_start_bar.is_finite()
            || plan.incoming_start_bar < 0.
            || plan.incoming_start_bar > summary.length_bars as f32
            || plan.clock.sample(plan.incoming_start_bar) >= plan.duration_sec()
        {
            return Err(invalid());
        }
        if !plan.master_bpm.nodes.is_empty() && !valid_line(&plan.master_bpm, 20., 800.) {
            return Err(invalid());
        }
        let last_bar = plan.clock.nodes.last().ok_or_else(invalid)?.0;
        if plan.stages.iter().any(|s| {
            !s.start_bar.is_finite()
                || !s.end_bar.is_finite()
                || s.start_bar < 0.
                || s.end_bar <= s.start_bar
                || s.end_bar > last_bar + 0.001
                || s.label.is_empty()
        }) || plan
            .stages
            .windows(2)
            .any(|s| s[1].start_bar < s[0].end_bar - 0.001)
        {
            return Err(invalid());
        }
        let lanes = &plan.lanes;
        if !valid_line(&lanes.xfader, -1.0, 1.0) {
            return Err(invalid());
        }
        for line in [
            &lanes.gain_a,
            &lanes.gain_b,
            &lanes.eq_a.low,
            &lanes.eq_a.mid,
            &lanes.eq_a.high,
            &lanes.eq_b.low,
            &lanes.eq_b.mid,
            &lanes.eq_b.high,
        ] {
            if !valid_line(line, -96.0, 12.0) {
                return Err(invalid());
            }
        }
        for line in [&lanes.rate_a, &lanes.rate_b] {
            if !valid_line(line, 0.5, 2.0) {
                return Err(invalid());
            }
        }
        for line in [&lanes.pitch_a, &lanes.pitch_b] {
            if !valid_line(line, -24.0, 24.0) {
                return Err(invalid());
            }
        }
        for line in [&lanes.fx_send_a, &lanes.fx_send_b] {
            if !valid_line(line, 0.0, 1.0) {
                return Err(invalid());
            }
        }
        for line in [
            &lanes.filter_a.lp_hz,
            &lanes.filter_a.hp_hz,
            &lanes.filter_b.lp_hz,
            &lanes.filter_b.hp_hz,
        ] {
            if !valid_line(line, 20.0, 20000.0) {
                return Err(invalid());
            }
        }
        for (i, op) in [&lanes.loop_a, &lanes.loop_b].into_iter().enumerate() {
            if let Some(op) = op {
                let slot = &shared.decks[if i == 0 { outgoing } else { 1 - outgoing }];
                if !op.on_bar.is_finite()
                    || !op.off_bar.is_finite()
                    || op.on_bar < 0.0
                    || op.off_bar <= op.on_bar
                    || op.length_src_frames == 0
                    || op.start_src_frame.saturating_add(op.length_src_frames)
                        > slot.frames.load(Ordering::Relaxed)
                {
                    return Err(invalid());
                }
            }
        }
        for (i, op) in [&lanes.scratch_a, &lanes.scratch_b].into_iter().enumerate() {
            if let Some(op) = op {
                let slot = &shared.decks[if i == 0 { outgoing } else { 1 - outgoing }];
                if !op.on_bar.is_finite()
                    || !op.peak_bar.is_finite()
                    || !op.off_bar.is_finite()
                    || op.on_bar < 0.0
                    || !(op.on_bar < op.peak_bar && op.peak_bar < op.off_bar)
                    || op.off_bar > summary.length_bars as f32
                    || op.start_src_frame >= slot.frames.load(Ordering::Relaxed)
                    || op.peak_delta_frames.unsigned_abs()
                        > (slot.src_sr.load(Ordering::Relaxed) as u64 * 2)
                    || (op.start_src_frame as i128 + op.peak_delta_frames as i128) < 0
                    || (op.start_src_frame as i128 + op.peak_delta_frames as i128)
                        >= slot.frames.load(Ordering::Relaxed) as i128
                {
                    return Err(invalid());
                }
            }
        }
        let sr = shared.sample_rate.load(Ordering::Relaxed);
        let a = &shared.decks[outgoing];
        let b = &shared.decks[1 - outgoing];
        if a.track_id.load(Ordering::Relaxed) != summary.pair.0.0 as u64
            || b.track_id.load(Ordering::Relaxed) != summary.pair.1.0 as u64
        {
            return Err(EngineError::Protocol("automix tracks changed"));
        }
        if b.playing.load(Ordering::Relaxed) {
            return Err(EngineError::Protocol(
                "incoming automix deck must be paused",
            ));
        }
        let start_frame = plan.t_in_a as f64 * a.src_sr.load(Ordering::Relaxed) as f64;
        if a.playhead_frames() > start_frame + 1.0 {
            return Err(EngineError::Protocol(
                "automix start is behind playhead; replan",
            ));
        }
        if plan.t_out_a as f64 * a.src_sr.load(Ordering::Relaxed) as f64
            > a.frames.load(Ordering::Relaxed) as f64 + 2.0
            || plan.t_end_b as f64 * b.src_sr.load(Ordering::Relaxed) as f64
                > b.frames.load(Ordering::Relaxed) as f64 + 2.0
        {
            return Err(invalid());
        }
        let total_frames = (plan.duration_sec() * sr as f32).ceil() as usize;
        if total_frames < STEP || total_frames > sr as usize * 1800 {
            return Err(invalid());
        }
        let upper = 18000.0f32.min(sr as f32 * 0.45);
        let gain_origin = a.gain_milli.load(Ordering::Relaxed) as f32 / 100. - 96.;
        let fader_a = a.fader.load(Ordering::Relaxed) as f32 / 1000.;
        let fader_b = b.fader.load(Ordering::Relaxed) as f32 / 1000.;
        let fader_match = 20. * (fader_a.max(0.001) / fader_b.max(0.001)).log10();
        let mut points = Vec::with_capacity(total_frames.div_ceil(STEP) + 1);
        for j in 0..=total_frames.div_ceil(STEP) {
            let u = bar_at(&plan.clock, (j * STEP).min(total_frames) as f32 / sr as f32);
            let mut decks = std::array::from_fn(|index| {
                let (gain, eq, filter, send, rate, pitch, loop_op) = if index == 0 {
                    (
                        &lanes.gain_a,
                        &lanes.eq_a,
                        &lanes.filter_a,
                        &lanes.fx_send_a,
                        &lanes.rate_a,
                        &lanes.pitch_a,
                        &lanes.loop_a,
                    )
                } else {
                    (
                        &lanes.gain_b,
                        &lanes.eq_b,
                        &lanes.filter_b,
                        &lanes.fx_send_b,
                        &lanes.rate_b,
                        &lanes.pitch_b,
                        &lanes.loop_b,
                    )
                };
                let lp = filter.lp_hz.sample_log(u);
                let hp = filter.hp_hz.sample_log(u);
                let amount = if lp < 19999.0 {
                    -(lp.clamp(30.0, upper) / upper).ln() / (30.0 / upper).ln()
                } else if hp > 20.01 {
                    (hp.clamp(30.0, upper) / 30.0).ln() / (upper / 30.0).ln()
                } else {
                    0.0
                };
                let modern = plan.transition_mode.is_some();
                let origin = if modern {
                    gain_origin + if index == 1 { fader_match } else { 0. }
                } else {
                    0.
                };
                let level = gain.sample(u) + origin;
                let trim = origin
                    + if index == 1 {
                        gain.nodes.last().map_or(0., |n| n.1)
                    } else {
                        0.
                    };
                let attenuation = if modern { (level - trim).min(0.) } else { 0. };
                let base_fader = if index == 0 { fader_a } else { fader_b };
                DeckPoint {
                    gain: db(level - attenuation),
                    fader: (base_fader * db_to_lin(attenuation) * 1000.).round() as u32,
                    eq: [
                        db(eq.low.sample(u)),
                        db(eq.mid.sample(u)),
                        db(eq.high.sample(u)),
                    ],
                    filter: ((amount + 1.0) * 500.0).round() as u32,
                    send: (send.sample(u) * 1000.0).round() as u32,
                    rate: (rate.sample(u) * 1_000_000.0).round() as u32,
                    pitch: ((pitch.sample(u) + 24.0) * 100.0).round() as u32,
                    loop_on: loop_op
                        .as_ref()
                        .is_some_and(|op| u >= op.on_bar && u < op.off_bar),
                    scratch_touch: {
                        let op = if index == 0 {
                            &lanes.scratch_a
                        } else {
                            &lanes.scratch_b
                        };
                        op.as_ref()
                            .is_some_and(|op| u >= op.on_bar && u < op.off_bar)
                    },
                    scratch_target: {
                        let op = if index == 0 {
                            &lanes.scratch_a
                        } else {
                            &lanes.scratch_b
                        };
                        if let Some(op) = op {
                            let phase = if u <= op.peak_bar {
                                (u - op.on_bar) / (op.peak_bar - op.on_bar)
                            } else {
                                (op.off_bar - u) / (op.off_bar - op.peak_bar)
                            };
                            (op.start_src_frame as i128
                                + (op.peak_delta_frames as f32 * phase.clamp(0.0, 1.0)) as i128)
                                .max(0) as u64
                        } else {
                            0
                        }
                    },
                }
            });
            let overlap = plan.clock.sample(summary.length_bars as f32) as f64;
            let sec = (j * STEP).min(total_frames) as f64 / sr as f64;
            if sec < overlap && !plan.incoming_source.nodes.is_empty() {
                let next_sec = ((j * STEP + STEP) as f64 / sr as f64).min(overlap);
                let rate = (source_at_seconds(&plan, &plan.incoming_source, next_sec)
                    - source_at_seconds(&plan, &plan.incoming_source, sec))
                    / (next_sec - sec);
                if !rate.is_finite() || !(0.4999..=2.0001).contains(&rate) {
                    return Err(invalid());
                }
                decks[1].rate = (rate.clamp(0.5, 2.0) * 1_000_000.0).round() as u32;
            }
            if sec < overlap && !plan.outgoing_source.nodes.is_empty() {
                let next_sec = ((j * STEP + STEP) as f64 / sr as f64).min(overlap);
                let rate = (source_at_seconds(&plan, &plan.outgoing_source, next_sec)
                    - source_at_seconds(&plan, &plan.outgoing_source, sec))
                    / (next_sec - sec);
                if !rate.is_finite() || !(0.4999..=2.0001).contains(&rate) {
                    return Err(invalid());
                }
                decks[0].rate = (rate.clamp(0.5, 2.0) * 1_000_000.0).round() as u32;
            }
            points.push(Point {
                tempo: plan.master_bpm.sample(u),
                xf: ((lanes.xfader.sample(u) + 1.0) * 500.0).round() as u32,
                decks,
            });
        }
        Ok(Self {
            start_frame,
            incoming_start_frames: (plan.clock.sample(plan.incoming_start_bar) * sr as f32).round()
                as usize,
            incoming_frame: (plan.t_in_b * b.src_sr.load(Ordering::Relaxed) as f32).round() as u64,
            overlap_frames: (plan.clock.sample(summary.length_bars as f32) * sr as f32).round()
                as usize,
            handoff_frame: plan
                .handoff_bar
                .map(|bar| (plan.clock.sample(bar) * sr as f32).round() as usize),
            total_frames,
            sample_rate: sr,
            plan,
            outgoing,
            points,
        })
    }
}

impl Engine {
    pub fn load_plan(&self, plan: MixPlan) -> Result<(), EngineError> {
        let summary = plan
            .summary
            .as_ref()
            .ok_or(EngineError::Protocol("no automix plan"))?;
        let outgoing =
            if self.shared.decks[0].track_id.load(Ordering::Relaxed) == summary.pair.0.0 as u64 {
                DeckId::A
            } else {
                DeckId::B
            };
        self.load_plan_on(plan, outgoing)
    }
    /// Explicit deck mapping also supports repeated tracks in a playlist.
    pub fn load_plan_on(&self, plan: MixPlan, outgoing: DeckId) -> Result<(), EngineError> {
        let prepared = self.prepare_plan_on(plan, outgoing)?;
        self.commit_plan(prepared)
    }

    pub fn prepare_plan_on(
        &self,
        plan: MixPlan,
        outgoing: DeckId,
    ) -> Result<PreparedMix, EngineError> {
        Ok(PreparedMix(Arc::new(DensePlan::compile(
            plan,
            outgoing.index(),
            &self.shared,
        )?)))
    }

    /// Short host publication step, separable from compilation for cancellation.
    pub fn commit_plan(&self, prepared: PreparedMix) -> Result<(), EngineError> {
        let dense = prepared.0;
        let pair = dense.plan.summary.as_ref().unwrap().pair;
        let a = &self.shared.decks[dense.outgoing];
        let b = &self.shared.decks[1 - dense.outgoing];
        if a.track_id.load(Ordering::Relaxed) != pair.0.0 as u64
            || b.track_id.load(Ordering::Relaxed) != pair.1.0 as u64
            || b.playing.load(Ordering::Relaxed)
            || dense.sample_rate != self.shared.sample_rate.load(Ordering::Relaxed)
        {
            return Err(EngineError::Protocol("automix decks changed"));
        }
        // The incoming deck visibly sits at its automatic cue before playback.
        a.automix_cue
            .store(dense.start_frame.round() as u64 + 1, Ordering::Relaxed);
        b.automix_cue
            .store(dense.incoming_frame + 1, Ordering::Relaxed);
        b.seek_to
            .store(dense.incoming_frame * 65536, Ordering::Relaxed);
        b.seek_pending.store(true, Ordering::Release);
        // Cue, tempo, key and tone are visible on the paused incoming deck well
        // before launch. Keep its channel closed until the scheduled first beat.
        let staged = dense.points[0].decks[1];
        b.rate_micro.store(staged.rate, Ordering::Relaxed);
        b.pitch_centi.store(staged.pitch, Ordering::Relaxed);
        b.keylock.store(true, Ordering::Relaxed);
        b.gain_milli.store(staged.gain, Ordering::Relaxed);
        b.fader.store(0, Ordering::Relaxed);
        b.filter_milli.store(staged.filter, Ordering::Relaxed);
        b.send_milli.store(0, Ordering::Relaxed);
        for band in 0..3 {
            b.eq_db[band].store(staged.eq[band], Ordering::Relaxed);
            b.eq_kill[band].store(false, Ordering::Relaxed);
        }
        let shared = &self.shared.automation;
        let mut published = shared
            .published
            .lock()
            .map_err(|_| EngineError::Protocol("automix lock poisoned"))?;
        let mut retired = shared
            .retired
            .lock()
            .map_err(|_| EngineError::Protocol("automix lock poisoned"))?;
        retired.retain(|p| Arc::strong_count(p) > 1);
        if let Some(old) = published.replace(dense) {
            retired.push(old);
        }
        shared.cancelled.store(0, Ordering::Release);
        shared.progress.store(0.0f32.to_bits(), Ordering::Relaxed);
        shared.skip.store(false, Ordering::Release);
        shared.paused.store(false, Ordering::Release);
        self.shared.clear_sync();
        shared.enabled.store(true, Ordering::Release);
        Ok(())
    }
    pub fn automix_summary(&self) -> Option<MixPlanSummary> {
        self.shared
            .automation
            .published
            .lock()
            .ok()?
            .as_ref()?
            .plan
            .summary
            .clone()
    }
    pub fn takeover_lane(&self, lane: LaneId) {
        let mask = match lane {
            LaneId::Xfader => XF,
            LaneId::GainA => bit(0, 0),
            LaneId::GainB => bit(1, 0),
            LaneId::EqA => bit(0, 1),
            LaneId::EqB => bit(1, 1),
            LaneId::FilterA => bit(0, 2),
            LaneId::FilterB => bit(1, 2),
            LaneId::SendA => bit(0, 3),
            LaneId::SendB => bit(1, 3),
            LaneId::RateA => bit(0, 4),
            LaneId::RateB => bit(1, 4),
            LaneId::PitchA => bit(0, 5),
            LaneId::PitchB => bit(1, 5),
            _ => 0,
        };
        self.shared
            .automation
            .cancelled
            .fetch_or(mask, Ordering::AcqRel);
    }
    pub(super) fn automation_command(&self, cmd: &Command) {
        let auto = &self.shared.automation;
        let mask = match *cmd {
            Command::SetCrossfader { .. }
            | Command::SetXfCurve { .. }
            | Command::SetXfReverse { .. } => XF,
            Command::SetChannelGain { deck, .. } | Command::SetChannelFader { deck, .. } => {
                bit(deck.index(), 0)
            }
            Command::SetEq { deck, .. } | Command::SetEqKill { deck, .. } => bit(deck.index(), 1),
            Command::SetFilter { deck, .. } | Command::SetChannelFilter { deck, .. } => {
                bit(deck.index(), 2)
            }
            Command::SetFxSend { deck, .. } => bit(deck.index(), 3),
            Command::SetRate { deck, .. } | Command::Sync { deck, .. } => bit(deck.index(), 4),
            Command::SetPitchSemitones { deck, .. } | Command::SetKeyLock { deck, .. } => {
                bit(deck.index(), 5)
            }
            Command::SetLoop { deck, .. }
            | Command::SetLoopBeats { deck, .. }
            | Command::LoopHalve { deck }
            | Command::LoopDouble { deck } => bit(deck.index(), 6),
            Command::PauseAutomix => {
                auto.paused.store(true, Ordering::Release);
                0
            }
            Command::ResumeAutomix => {
                auto.paused.store(false, Ordering::Release);
                0
            }
            Command::SkipAutomix => {
                auto.skip.store(true, Ordering::Release);
                auto.paused.store(false, Ordering::Release);
                0
            }
            Command::StopAutomix
            | Command::PlayPause { .. }
            | Command::JumpCue { .. }
            | Command::TriggerTemporaryCue { .. }
            | Command::SetBrake { on: true, .. }
            | Command::BeatJump { .. }
            | Command::SetReverse { .. }
            | Command::SetJogTouch { touching: true, .. } => {
                auto.enabled.store(false, Ordering::Release);
                auto.paused.store(false, Ordering::Release);
                for slot in &self.shared.decks {
                    slot.automix_cue.store(0, Ordering::Relaxed);
                }
                0
            }
            _ => 0,
        };
        auto.cancelled.fetch_or(mask, Ordering::AcqRel);
    }
}
impl Shared {
    fn automation_tick(&self, rt: &mut AutomationRt) -> bool {
        let auto = &self.automation;
        if !auto.enabled.load(Ordering::Acquire) {
            if rt.paused {
                if let Some(plan) = rt.plan.as_ref() {
                    let pair = plan.plan.summary.as_ref().unwrap().pair;
                    for (index, id) in [(plan.outgoing, pair.0), (1 - plan.outgoing, pair.1)] {
                        if self.decks[index].track_id.load(Ordering::Relaxed) == id.0 as u64 {
                            self.decks[index]
                                .playing
                                .store(rt.resume_playing[index], Ordering::Relaxed);
                        }
                    }
                }
                rt.paused = false;
            }
            rt.clock_deck = None;
            rt.tempo = None;
            return false;
        }
        if let Ok(published) = auto.published.try_lock() {
            if let Some(plan) = published.as_ref() {
                if rt.plan.as_ref().is_none_or(|old| !Arc::ptr_eq(old, plan)) {
                    rt.plan = Some(plan.clone());
                    rt.elapsed = 0;
                    rt.started = false;
                    rt.incoming_started = false;
                    rt.paused = false;
                    rt.clock_deck = None;
                }
            }
        }
        let Some(plan) = rt.plan.as_ref() else {
            return false;
        };
        let a = &self.decks[plan.outgoing];
        let b = &self.decks[1 - plan.outgoing];
        let summary = plan.plan.summary.as_ref().unwrap();
        if plan.sample_rate != self.sample_rate.load(Ordering::Relaxed)
            || a.track_id.load(Ordering::Relaxed) != summary.pair.0.0 as u64
            || b.track_id.load(Ordering::Relaxed) != summary.pair.1.0 as u64
        {
            auto.enabled.store(false, Ordering::Release);
            return false;
        }
        let paused = auto.paused.load(Ordering::Acquire);
        if paused && !rt.paused {
            for i in 0..2 {
                rt.resume_playing[i] = self.decks[i].playing.swap(false, Ordering::AcqRel);
            }
            rt.paused = true;
        } else if !paused && rt.paused {
            for i in 0..2 {
                self.decks[i]
                    .playing
                    .store(rt.resume_playing[i], Ordering::Relaxed);
            }
            rt.paused = false;
        }
        if paused {
            return false;
        }
        let skip = auto.skip.swap(false, Ordering::AcqRel);
        if !rt.started {
            if !a.playing.load(Ordering::Relaxed) {
                return false;
            }
            if !skip && a.playhead_frames() + 0.5 < plan.start_frame {
                return false;
            }
            rt.started = true;
            if plan
                .plan
                .lanes
                .fx_send_a
                .nodes
                .iter()
                .any(|(_, v)| *v > 0.0)
            {
                self.sends[0]
                    .beats
                    .store(1.0f32.to_bits(), Ordering::Relaxed);
                self.sends[0].mix.store(1.0f32.to_bits(), Ordering::Relaxed);
                self.sends[0].bypass.store(false, Ordering::Relaxed);
            }
            self.xf_curve.store(1, Ordering::Relaxed);
            self.xf_reverse.store(false, Ordering::Relaxed);

            for slot in [a, b] {
                slot.keylock.store(true, Ordering::Relaxed);
                slot.reverse.store(false, Ordering::Relaxed);
            }
            if plan.plan.lanes.scratch_a.is_some() {
                a.slip.store(true, Ordering::Relaxed);
            }
            if plan.plan.lanes.scratch_b.is_some() {
                b.slip.store(true, Ordering::Relaxed);
            }
        }
        if skip {
            rt.elapsed = plan.total_frames;
        }
        // Source EOF may precede the next 32-frame automation sample by a
        // fraction of a block. A natural end must hand off to B, never freeze
        // the clock just short of its launch. Manual pause already cancels AUTO.
        if !a.playing.load(Ordering::Relaxed)
            && a.playhead_frames() + 1. >= a.frames.load(Ordering::Relaxed) as f64
            && rt.elapsed < plan.overlap_frames
        {
            rt.elapsed = plan.overlap_frames;
        }
        if !rt.incoming_started && rt.elapsed >= plan.incoming_start_frames {
            b.seek_to
                .store(plan.incoming_frame * 65536, Ordering::Relaxed);
            b.seek_pending.store(true, Ordering::Release);
            b.playing.store(true, Ordering::Relaxed);
            rt.incoming_started = true;
        }
        let point = plan.points[if rt.elapsed >= plan.total_frames {
            plan.points.len() - 1
        } else {
            rt.elapsed / STEP
        }];
        rt.tempo = (point.tempo > 0.).then_some(point.tempo);
        let cancelled = auto.cancelled.load(Ordering::Acquire);
        if cancelled & XF == 0 {
            self.xfader.store(
                if plan.outgoing == 0 {
                    point.xf
                } else {
                    1000 - point.xf
                },
                Ordering::Relaxed,
            );
        }
        for (relative, p) in point.decks.into_iter().enumerate() {
            let index = if relative == 0 {
                plan.outgoing
            } else {
                1 - plan.outgoing
            };
            let s = &self.decks[index];
            if cancelled & bit(index, 0) == 0 {
                s.gain_milli.store(p.gain, Ordering::Relaxed);
                s.fader.store(p.fader, Ordering::Relaxed);
            }
            if cancelled & bit(index, 1) == 0 {
                for band in 0..3 {
                    s.eq_db[band].store(p.eq[band], Ordering::Relaxed);
                    s.eq_kill[band].store(false, Ordering::Relaxed);
                }
            }
            if cancelled & bit(index, 2) == 0 {
                s.filter_milli.store(p.filter, Ordering::Relaxed);
            }
            if cancelled & bit(index, 3) == 0 {
                s.send_milli.store(p.send, Ordering::Relaxed);
            }
            if cancelled & bit(index, 4) == 0 {
                s.rate_micro.store(p.rate, Ordering::Relaxed);
            }
            if cancelled & bit(index, 5) == 0 {
                s.pitch_centi.store(p.pitch, Ordering::Relaxed);
            }
            if cancelled & bit(index, 6) == 0 {
                let op = if relative == 0 {
                    &plan.plan.lanes.loop_a
                } else {
                    &plan.plan.lanes.loop_b
                };
                if let Some(op) = op {
                    s.loop_start
                        .store(op.start_src_frame * 65536, Ordering::Relaxed);
                    s.loop_length_frames
                        .store(op.length_src_frames, Ordering::Relaxed);
                    s.loop_sixteenths
                        .store(op.length_bars as u32 * 64, Ordering::Relaxed);
                }
                s.loop_on.store(p.loop_on, Ordering::Relaxed);
            }
            if p.scratch_touch {
                s.jog_target
                    .store(p.scratch_target * 65536, Ordering::Release);
                s.jog_touch.store(true, Ordering::Release);
            } else if (relative == 0 && plan.plan.lanes.scratch_a.is_some())
                || (relative == 1 && plan.plan.lanes.scratch_b.is_some())
            {
                s.jog_touch.store(false, Ordering::Release);
            }
        }
        rt.clock_deck = Some(
            if plan.handoff_frame.is_some_and(|frame| rt.elapsed >= frame) {
                1 - plan.outgoing
            } else {
                plan.outgoing
            },
        );
        if rt.elapsed >= plan.overlap_frames {
            a.playing.store(false, Ordering::Relaxed);
        }
        auto.progress.store(
            (rt.elapsed as f32 / plan.total_frames as f32)
                .min(1.0)
                .to_bits(),
            Ordering::Relaxed,
        );
        if rt.elapsed >= plan.total_frames {
            if plan.plan.lanes.scratch_a.is_some() {
                a.jog_touch.store(false, Ordering::Release);
                a.slip.store(false, Ordering::Relaxed);
            }
            if plan.plan.lanes.scratch_b.is_some() {
                b.jog_touch.store(false, Ordering::Release);
                b.slip.store(false, Ordering::Relaxed);
            }
            auto.enabled.store(false, Ordering::Release);
            return false;
        }
        // A paused transport freezes the clock before handoff; B owns it after.
        if rt.elapsed < plan.overlap_frames {
            a.playing.load(Ordering::Relaxed)
        } else {
            b.playing.load(Ordering::Relaxed)
        }
    }
    pub(super) fn process_block(&self, rt: &mut AudioRt, out: &mut [f32], channels: usize) {
        if !self.automation.enabled.load(Ordering::Acquire) && !rt.automation.paused {
            self.process_audio_block(rt, out, channels);
            return;
        }
        for chunk in out.chunks_mut(STEP * channels.max(1)) {
            let advance = self.automation_tick(&mut rt.automation);
            self.process_audio_block(rt, chunk, channels);
            if advance {
                rt.automation.elapsed += chunk.len() / channels.max(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{PerformanceOffset, TempoMap, TempoSegment, TrackAnalysis, TrackId};
    fn analysis(id: i64, sr: u32, seconds: f32) -> TrackAnalysis {
        TrackAnalysis {
            track_id: TrackId(id),
            duration_sec: seconds,
            sample_rate: sr,
            tempo: TempoMap {
                global_bpm: 120.,
                meter_num: 1,
                meter_den: 4,
                segments: vec![TempoSegment {
                    start_beat: 0.,
                    end_beat: seconds * 2.,
                    bpm: 120.,
                    confidence: 1.,
                }],
                beats: vec![],
                downbeats: vec![],
            },
            key: None,
            camelot: None,
            key_confidence: 0.,
            sections: vec![],
            bars: vec![],
            phrase_boundaries: vec![],
            mix_regions: vec![],
            waveform_path: None,
            partial: true,
        }
    }
    fn setup(outgoing: DeckId) -> (Engine, MixPlan) {
        let engine = super::super::tests::test_engine(48000);
        engine.shared.decks[outgoing.index()]
            .track_id
            .store(1, Ordering::Relaxed);
        engine.shared.decks[1 - outgoing.index()]
            .track_id
            .store(2, Ordering::Relaxed);
        let a = analysis(1, 48000, 1.);
        let b = analysis(2, 48000, 4.);
        let plan = mixless_mixplan::Planner::with_options(mixless_mixplan::PlannerOptions {
            smooth: false,
            earliest_outgoing_sec: 0.5,
            ..Default::default()
        })
        .plan_pair(
            &a,
            &b,
            &[],
            &[],
            PerformanceOffset::identity(),
            PerformanceOffset::identity(),
        );
        assert!(plan.summary.is_some());
        engine
            .dispatch(Command::SetCrossfader {
                value: if outgoing == DeckId::A { -1. } else { 1. },
            })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: outgoing })
            .unwrap();
        (engine, plan)
    }
    #[test]
    fn automix_waits_then_renders_and_hands_off_both_deck_directions() {
        for outgoing in [DeckId::A, DeckId::B] {
            let (engine, plan) = setup(outgoing);
            engine.load_plan_on(plan, outgoing).unwrap();
            let before = engine.render_offline(12000);
            assert!(before.iter().any(|v| v.abs() > 0.01));
            assert!(!engine.snapshot().decks[1 - outgoing.index()].playing);
            let mixed = engine.render_offline(48000);
            assert!(mixed.iter().all(|v| v.is_finite() && v.abs() <= 1.));
            assert!(mixed.iter().any(|v| v.abs() > 0.01));
            let snapshot = engine.snapshot();
            assert!(!snapshot.automix_on);
            assert_eq!(snapshot.automix_progress, 1.);
            assert!(!snapshot.deck(outgoing).playing);
            assert!(snapshot.decks[1 - outgoing.index()].playing);
            assert_eq!(
                snapshot.xfader,
                if outgoing == DeckId::A { 1. } else { -1. }
            );
            assert_eq!(snapshot.deck(outgoing).gain_db, -96.);
        }
    }
    #[test]
    fn disabling_auto_during_a_blend_keeps_both_transports_and_controls() {
        let (engine, plan) = setup(DeckId::A);
        engine.load_plan(plan).unwrap();
        engine.render_offline(30000);
        let playing = engine.snapshot();
        assert!(playing.decks.iter().all(|d| d.playing));
        engine.dispatch(Command::StopAutomix).unwrap();
        engine.render_offline(4800);
        let manual = engine.snapshot();
        assert!(!manual.automix_on && manual.decks.iter().all(|d| d.playing));
        assert_eq!(manual.xfader, playing.xfader);
        assert_eq!(
            manual.decks.each_ref().map(|d| d.fader),
            playing.decks.each_ref().map(|d| d.fader)
        );
        assert!(
            manual
                .decks
                .iter()
                .zip(&playing.decks)
                .all(|(a, b)| a.frame > b.frame)
        );
    }
    #[test]
    fn automix_pause_resume_and_single_lane_takeover() {
        let (engine, plan) = setup(DeckId::A);
        engine.load_plan(plan).unwrap();
        engine.render_offline(30000);
        engine.dispatch(Command::PauseAutomix).unwrap();
        engine.render_offline(256);
        let paused = engine.snapshot();
        engine.render_offline(12000);
        let still = engine.snapshot();
        assert_eq!(still.automix_progress, paused.automix_progress);
        assert_eq!(still.xfader, paused.xfader);
        assert!(!still.decks[0].playing && !still.decks[1].playing);
        engine
            .dispatch(Command::SetCrossfader { value: -0.8 })
            .unwrap();
        engine
            .dispatch(Command::SetEq {
                deck: DeckId::B,
                band: mixless_protocol::EqBand::Low,
                db: -9.,
            })
            .unwrap();
        engine.dispatch(Command::ResumeAutomix).unwrap();
        engine.render_offline(30000);
        let snapshot = engine.snapshot();
        assert_eq!(snapshot.xfader, -0.8);
        assert_eq!(snapshot.decks[1].eq_db[0], -9.);
        assert_eq!(snapshot.decks[0].gain_db, -96.);
        assert!(!snapshot.automix_on);
    }
    #[test]
    fn automix_skip_stop_invalid_and_stale_plan() {
        let (engine, plan) = setup(DeckId::A);
        let mut broken = plan.clone();
        broken.lanes.rate_b.nodes[0].1 = f32::NAN;
        assert!(engine.load_plan(broken).is_err());
        assert!(!engine.snapshot().automix_on);
        engine.load_plan(plan.clone()).unwrap();
        engine.dispatch(Command::SkipAutomix).unwrap();
        engine.render_offline(64);
        let snap = engine.snapshot();
        assert!(!snap.automix_on);
        assert_eq!(snap.automix_progress, 1.);
        assert!(snap.decks[1].playing);
        let (engine, _) = setup(DeckId::A);
        engine.load_plan(plan).unwrap();
        engine.dispatch(Command::StopAutomix).unwrap();
        engine.render_offline(48000);
        assert!(!engine.snapshot().decks[1].playing);
        let (engine, plan) = setup(DeckId::A);
        engine.load_plan(plan).unwrap();
        engine.shared.decks[1].track_id.store(9, Ordering::Relaxed);
        engine.render_offline(64);
        assert!(!engine.snapshot().automix_on);
    }
    #[test]
    fn automix_published_buffer_contention_keeps_current_plan() {
        let (engine, plan) = setup(DeckId::A);
        engine.load_plan(plan).unwrap();
        engine.render_offline(256);
        let _guard = engine.shared.automation.published.lock().unwrap();
        engine.render_offline(60000);
        assert_eq!(engine.snapshot().automix_progress, 1.);
    }

    #[test]
    fn smooth_bridge_launches_incoming_at_outgoing_end_in_both_directions() {
        for outgoing in [DeckId::A, DeckId::B] {
            let (engine, _) = setup(outgoing);
            let a = analysis(1, 48000, 1.);
            let mut b = analysis(2, 48000, 4.);
            b.tempo.global_bpm = 97.;
            b.tempo.segments[0].bpm = 97.;
            let plan = mixless_mixplan::Planner::new().plan_pair(
                &a,
                &b,
                &[],
                &[],
                Default::default(),
                Default::default(),
            );
            let boundary = ((plan.t_in_a + plan.clock.sample(plan.incoming_start_bar)) * 48000.)
                .round() as usize;
            assert_eq!(
                plan.incoming_start_bar,
                plan.summary.as_ref().unwrap().length_bars as f32
            );
            engine.load_plan_on(plan, outgoing).unwrap();
            engine.render_offline(boundary);
            assert!(!engine.snapshot().decks[1 - outgoing.index()].playing);
            let start = engine.render_offline(4800);
            let snap = engine.snapshot();
            assert!(!snap.deck(outgoing).playing);
            assert!(snap.decks[1 - outgoing.index()].playing);
            assert!(snap.decks[1 - outgoing.index()].frame.abs_diff(4800) < 64);
            assert!(start.iter().any(|s| s.abs() > 0.05));
            engine.render_offline(48000);
            assert_eq!(engine.snapshot().automix_progress, 1.);
        }
    }

    #[test]
    fn smooth_render_preserves_beat_phase_across_source_rates_and_deck_directions() {
        use mixless_protocol::{BarFeature, Section, SectionLabel, TransitionMode};
        for outgoing in [DeckId::A, DeckId::B] {
            let engine = super::super::tests::test_engine(48000);
            let mut analyses = Vec::new();
            for (relative, (sr, bpm)) in [(44100, 128.), (48000, 132.)].into_iter().enumerate() {
                let index = if relative == 0 {
                    outgoing.index()
                } else {
                    1 - outgoing.index()
                };
                let duration = 64. * 240. / bpm;
                let frames = (duration * sr as f32).ceil() as usize;
                let mut samples = Vec::with_capacity(frames * 2);
                for i in 0..frames {
                    let t = i as f32 / sr as f32;
                    let beat_time = (t * bpm / 60.).fract() * 60. / bpm;
                    let sample = 0.15
                        * (-beat_time * 40.).exp()
                        * (std::f32::consts::TAU * 70. * beat_time).sin()
                        + 0.05 * (std::f32::consts::TAU * 220. * t).sin();
                    samples.extend([sample, sample]);
                }
                let slot = &engine.shared.decks[index];
                slot.track_id.store(relative as u64 + 1, Ordering::Relaxed);
                slot.src_sr.store(sr, Ordering::Relaxed);
                slot.frames.store(frames as u64, Ordering::Relaxed);
                *slot.buffer.lock().unwrap() = Some(Arc::new(crate::decode::AudioBuffer {
                    samples,
                    frames: frames as u64,
                    sample_rate: sr,
                }));
                let mut a = analysis(relative as i64 + 1, sr, duration);
                a.tempo.global_bpm = bpm;
                a.tempo.meter_num = 4;
                a.tempo.segments[0].bpm = bpm;
                a.tempo.segments[0].end_beat = 256.;
                a.tempo.beats = (0..=256).map(|i| i as f32 * 60. / bpm).collect();
                a.tempo.downbeats = a.tempo.beats.iter().step_by(4).copied().collect();
                a.camelot = Some(if relative == 0 { "8A" } else { "3A" }.into());
                a.key_confidence = 1.;
                a.sections = vec![Section {
                    start_sec: 0.,
                    end_sec: duration,
                    label: SectionLabel::Intro,
                }];
                a.bars = (0..64)
                    .map(|i| BarFeature {
                        bar_index: i,
                        start_sec: i as f32 * 240. / bpm,
                        end_sec: (i + 1) as f32 * 240. / bpm,
                        rms: 0.05,
                        crest: 3.,
                        low_db: -25.,
                        mid_db: -30.,
                        high_db: -60.,
                        chroma: [0.; 12],
                        chord: None,
                        local_key: None,
                        onset_density: 2.,
                        kick_salience: 1.,
                        hat_salience: 0.,
                        vocal_presence: 0.,
                        vocal_confidence: None,
                        energy_slope: 0.,
                        section: SectionLabel::Intro,
                    })
                    .collect();
                analyses.push(a);
            }
            let plan = mixless_mixplan::Planner::with_options(mixless_mixplan::PlannerOptions {
                harmonic_key_shift: true,
                earliest_outgoing_sec: 1.,
                ..Default::default()
            })
            .plan_pair(
                &analyses[0],
                &analyses[1],
                &[],
                &[],
                Default::default(),
                Default::default(),
            );
            assert_eq!(plan.transition_mode, Some(TransitionMode::BeatBlend));
            engine.shared.decks[outgoing.index()]
                .set_playhead((plan.t_in_a as f64 - 0.25) * 44100.);
            engine
                .dispatch(Command::SetChannelGain {
                    deck: outgoing,
                    db: -3.,
                })
                .unwrap();
            engine
                .dispatch(Command::SetCrossfader {
                    value: if outgoing == DeckId::A { -1. } else { 1. },
                })
                .unwrap();
            engine
                .dispatch(Command::PlayPause { deck: outgoing })
                .unwrap();
            engine.load_plan_on(plan.clone(), outgoing).unwrap();
            let staged = engine.snapshot();
            let b = &staged.decks[1 - outgoing.index()];
            assert!(!b.playing && b.fader == 0. && b.keylock);
            assert_eq!(b.pitch_semitones, -1.);
            assert!((b.rate - 128. / 132.).abs() < 0.0001);
            engine.render_offline(6000);
            let waiting = engine.snapshot();
            let b = &waiting.decks[1 - outgoing.index()];
            assert!(!b.playing && b.fader == 0.);
            assert!(b.frame.abs_diff((plan.t_in_b * 48000.).round() as u64) < 2);
            engine.render_offline(6000);
            let total = (plan.duration_sec() * 48000.).ceil() as usize;
            let mut max_phase_error = 0f64;
            for start in (0..total).step_by(1024) {
                let audio = engine.render_offline((total - start).min(1024));
                assert!(audio.iter().all(|s| s.is_finite() && s.abs() <= 1.));
                let snap = engine.snapshot();
                let beat_a =
                    (snap.deck(outgoing).frame as f64 / 44100. - plan.t_in_a as f64) * 128. / 60.;
                let beat_b = (snap.decks[1 - outgoing.index()].frame as f64 / 48000.
                    - plan.t_in_b as f64)
                    * 132.
                    / 60.;
                max_phase_error = max_phase_error.max((beat_a - beat_b).abs());
            }
            assert!(
                max_phase_error < 0.015,
                "beat phase error {max_phase_error}"
            );
            engine.render_offline(64);
            let snap = engine.snapshot();
            assert_eq!(snap.automix_progress, 1.);
            assert!(!snap.deck(outgoing).playing && snap.deck(outgoing).fader == 0.);
            assert!(
                snap.decks[1 - outgoing.index()].playing
                    && snap.decks[1 - outgoing.index()].fader > 0.
            );
            assert_eq!(snap.decks[1 - outgoing.index()].pitch_semitones, -1.);
            assert!((snap.decks[1 - outgoing.index()].rate - 1.).abs() < 0.00001);
            assert!((snap.decks[1 - outgoing.index()].gain_db + 3.).abs() < 0.011);
            eprintln!("{outgoing:?}: maximum rendered beat phase error {max_phase_error:.6} beats");
        }
    }
}
