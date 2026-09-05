use serde::{Deserialize, Serialize};

use crate::{
    AnalysisStage, DeckId, EngineSnapshot, ErrorCode, LaneId, MixPlanSummary, TrackId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    EngineSnapshot {
        snapshot: EngineSnapshot,
    },
    Meter {
        deck: Option<DeckId>,
        rms: [f32; 2],
        peak: [f32; 2],
    },
    Xrun {
        count: u64,
        last_block: u32,
    },
    AnalysisProgress {
        track_id: TrackId,
        stage: AnalysisStage,
        pct: f32,
    },
    AnalysisReady {
        track_id: TrackId,
    },
    MixPlanReady {
        plan: MixPlanSummary,
    },
    AutomixTakeover {
        lane: LaneId,
    },
    SpotifyStatus {
        connected: bool,
        display_name: Option<String>,
        premium: bool,
    },
    LibraryChanged {
        reason: String,
    },
    Error {
        code: ErrorCode,
        message: String,
    },
}
