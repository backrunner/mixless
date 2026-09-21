use serde::{Deserialize, Serialize};

use crate::{DeckId, TrackId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyId {
    DryCut,
    PhraseBlend,
    BassSwap,
    DropCut,
    EchoOut,
    FilterSweep,
    LoopConstruct,
    ScratchCut,
    Spinback,
    LoopOut,
    BreakToIntro,
    EnergyHold,
    FallbackSwapFilter,
}

impl StrategyId {
    pub fn default_bars(self) -> u16 {
        match self {
            Self::PhraseBlend | Self::BreakToIntro => 32,
            Self::DryCut | Self::DropCut | Self::ScratchCut | Self::Spinback => 4,
            Self::EchoOut | Self::LoopOut => 8,
            _ => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PerformanceOffset {
    pub rate: f32,
    pub pitch_semitones: f32,
}

impl Default for PerformanceOffset {
    fn default() -> Self {
        Self::identity()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum BarMap {
    #[default]
    OneToOne,
    TwoToOne {
        outgoing_is_double: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WhoStretches {
    A,
    #[default]
    B,
    Both,
}

/// How much between-transition live motion AutoMix applies to the playing deck.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LiveMoves {
    Off,
    #[default]
    Subtle,
    Active,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionMode {
    /// Reliable grids, bounded shared tempo migration, downbeat bass handoff.
    BeatBlend,
    /// Separate rhythmic/tonal foregrounds; tracks keep their own tempo/key.
    PhraseBridge,
    /// Quantized incoming loop roll; selected only with reliable grids.
    LoopRoll,
}

/// Coordinates are outgoing master bars; filter lanes interpolate in log Hz.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Polyline {
    pub nodes: Vec<(f32, f32)>,
}

impl Polyline {
    pub fn constant(value: f32) -> Self {
        Self {
            nodes: vec![(0.0, value)],
        }
    }

    pub fn sample(&self, bar: f32) -> f32 {
        self.interpolate(bar, false)
    }
    pub fn sample_log(&self, bar: f32) -> f32 {
        self.interpolate(bar, true)
    }

    fn interpolate(&self, bar: f32, logarithmic: bool) -> f32 {
        let Some(&(first, value)) = self.nodes.first() else {
            return 0.0;
        };
        if bar <= first {
            return value;
        }
        let next = self.nodes.partition_point(|(u, _)| *u <= bar);
        if next > 0 && next < self.nodes.len() {
            let (a, x) = self.nodes[next - 1];
            let (b, y) = self.nodes[next];
            let t = ((bar - a) / (b - a)).clamp(0.0, 1.0);
            return if logarithmic {
                (x.max(1.0).ln() + t * (y.max(1.0).ln() - x.max(1.0).ln())).exp()
            } else {
                x + t * (y - x)
            };
        }
        self.nodes.last().unwrap().1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EqLane {
    pub low: Polyline,
    pub mid: Polyline,
    pub high: Polyline,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilterLane {
    pub lp_hz: Polyline,
    pub hp_hz: Polyline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOp {
    pub start_src_frame: u64,
    pub length_bars: u16,
    pub on_bar: f32,
    pub off_bar: f32,
    /// Exact source-domain length, also valid for non-4/4 and local tempo maps.
    pub length_src_frames: u64,
}

/// A short, quantized vinyl-style cut. The deck is touched at `on_bar`,
/// jogged back by `peak_delta_frames` at `peak_bar`, then released at
/// `off_bar`; slip mode returns the playhead to the original timeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScratchOp {
    pub start_src_frame: u64,
    pub peak_delta_frames: i64,
    pub on_bar: f32,
    pub peak_bar: f32,
    pub off_bar: f32,
    /// Accelerating pull (a spinback eases in, then whips back fastest just
    /// before release); a steady jog stays linear.
    #[serde(default)]
    pub accelerate: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutomationLanes {
    pub xfader: Polyline,
    pub gain_a: Polyline,
    pub gain_b: Polyline,
    pub eq_a: EqLane,
    pub eq_b: EqLane,
    pub filter_a: FilterLane,
    pub filter_b: FilterLane,
    pub fx_send_a: Polyline,
    pub fx_send_b: Polyline,
    pub rate_a: Polyline,
    pub rate_b: Polyline,
    pub pitch_a: Polyline,
    pub pitch_b: Polyline,
    pub loop_a: Option<LoopOp>,
    pub loop_b: Option<LoopOp>,
    #[serde(default)]
    pub scratch_a: Option<ScratchOp>,
    #[serde(default)]
    pub scratch_b: Option<ScratchOp>,
}

impl PerformanceOffset {
    pub fn identity() -> Self {
        Self {
            rate: 1.0,
            pitch_semitones: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixPlanSummary {
    pub pair: (TrackId, TrackId),
    pub strategy: StrategyId,
    pub score: f32,
    pub used_fallback: bool,
    pub length_bars: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MixStage {
    pub start_bar: f32,
    pub end_bar: f32,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MixPlan {
    /// Optional voice/drum/instrument envelopes; host uses these only with aligned PCM.
    #[serde(default)]
    pub stem_mix: Option<StemMix>,
    /// Human-readable stages corresponding to the actual control envelopes.
    #[serde(default)]
    pub stages: Vec<MixStage>,
    pub summary: Option<MixPlanSummary>,
    pub incoming_offset_end: PerformanceOffset,
    #[serde(default)]
    pub outgoing_offset: PerformanceOffset,
    #[serde(default)]
    pub bar_map: BarMap,
    /// Source seconds, never analysis-resampling frames.
    #[serde(default)]
    pub t_in_a: f32,
    #[serde(default)]
    pub t_out_a: f32,
    #[serde(default)]
    pub t_in_b: f32,
    #[serde(default)]
    pub t_end_b: f32,
    /// (master bar, wall seconds), integrated through the local tempo map.
    #[serde(default)]
    pub clock: Polyline,
    /// Incoming unwrapped source seconds at each master-grid node. Used to
    /// integrate the warp without accumulating tempo-boundary interpolation drift.
    #[serde(default)]
    pub incoming_source: Polyline,
    /// Outgoing source seconds on the same wall clock (for smooth tempo migration).
    #[serde(default)]
    pub outgoing_source: Polyline,
    #[serde(default)]
    pub incoming_start_bar: f32,
    #[serde(default)]
    pub transition_mode: Option<TransitionMode>,
    /// Sounding master tempo used by the mixer's tempo-synced effects.
    #[serde(default)]
    pub master_bpm: Polyline,
    #[serde(default)]
    pub lanes: AutomationLanes,
    /// Energy hold hands master ownership to incoming here; clock stays outgoing.
    #[serde(default)]
    pub handoff_bar: Option<f32>,
    #[serde(default)]
    pub literal_half_double: bool,
    /// An impossible user range yields an explicit failure, never an unsafe fallback.
    #[serde(default)]
    pub failure_reason: Option<String>,
    /// Stem-layered plans are tonal only because stem envelopes suppress the
    /// clash; without aligned stem PCM on both decks the engine must refuse them.
    #[serde(default)]
    pub requires_stems: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StemMix {
    pub outgoing: [Polyline; 3],
    pub incoming: [Polyline; 3],
}

impl MixPlan {
    pub fn duration_sec(&self) -> f32 {
        self.clock.nodes.last().map_or(0.0, |n| n.1)
    }
    pub fn stage_at_elapsed(&self, seconds: f32) -> Option<&MixStage> {
        self.stages.iter().find(|stage| {
            seconds >= self.clock.sample(stage.start_bar)
                && seconds < self.clock.sample(stage.end_bar)
        })
    }
    pub fn master_at(&self, bar: f32) -> DeckId {
        if self.handoff_bar.is_some_and(|at| bar >= at) {
            DeckId::B
        } else {
            DeckId::A
        }
    }
}
