//! Offline analysis. Must not open audio devices.

mod features;
pub use features::ANALYSIS_VERSION;

use mixless_engine::decode_file;
use mixless_protocol::{TempoMap, TempoSegment, TrackAnalysis, TrackId};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error("{0}")]
    Decode(String),
}

pub struct Analyzer;

pub struct AnalysisJob {
    pub track_id: TrackId,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct QuickAnalysis {
    pub bpm: f32,
    pub key: String,
    pub camelot: String,
    pub confidence: f32,
}

impl Analyzer {
    pub fn new() -> Self {
        Self
    }

    pub fn enqueue(&self, _job: AnalysisJob) {}

    pub fn cancel(&self, _track_id: TrackId) {}

    pub fn analyze_path(&self, path: &Path) -> Result<QuickAnalysis, AnalyzeError> {
        let analysis = self.analyze_track(TrackId(0), path)?;
        Ok(QuickAnalysis {
            bpm: analysis.tempo.global_bpm,
            key: analysis.key.unwrap_or_else(|| "Unknown".into()),
            camelot: analysis.camelot.unwrap_or_else(|| "Unknown".into()),
            confidence: analysis.key_confidence,
        })
    }

    /// Independent offline decode; returns source-rate cue coordinates, measured
    /// bar energy, a conservative tonal/vocal-risk proxy and grid confidence.
    pub fn analyze_track(&self, id: TrackId, path: &Path) -> Result<TrackAnalysis, AnalyzeError> {
        let buf = decode_file(path).map_err(|e| AnalyzeError::Decode(e.to_string()))?;
        Ok(features::analyze(id, &buf.samples, buf.sample_rate))
    }

    pub fn to_track_analysis(
        &self,
        id: TrackId,
        q: &QuickAnalysis,
        duration_sec: f32,
    ) -> TrackAnalysis {
        TrackAnalysis {
            track_id: id,
            duration_sec,
            sample_rate: 22_050,
            tempo: TempoMap {
                global_bpm: q.bpm,
                meter_num: 4,
                meter_den: 4,
                segments: vec![TempoSegment {
                    start_beat: 0.0,
                    end_beat: (duration_sec * q.bpm / 60.0).max(1.0),
                    bpm: q.bpm,
                    confidence: q.confidence,
                }],
                beats: vec![],
                downbeats: vec![],
            },
            key: Some(q.key.clone()),
            camelot: Some(q.camelot.clone()),
            key_confidence: q.confidence,
            sections: vec![],
            bars: vec![],
            waveform_path: None,
            partial: true,
        }
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}
