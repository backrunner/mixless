//! Offline analysis. Must not open audio devices.

mod cues;
pub use cues::automatic_cues;
mod features;
mod regions;
mod structure;
pub use features::ANALYSIS_VERSION;

mod sound_analysis;

use mixless_engine::decode_file;
use mixless_protocol::{TempoMap, TempoSegment, TrackAnalysis, TrackId};
use std::{
    path::Path,
    time::{Duration, Instant},
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AnalyzeError {
    #[error("{0}")]
    Decode(String),
}

#[derive(Debug, Clone)]
pub struct AnalysisOptions {
    /// Apple's on-device human-voice classifier (macOS 12+); never required.
    pub native_vocals: bool,
    /// Cooperative budget, checked between audio chunks; capped at five seconds.
    pub vocal_budget: Duration,
}
impl Default for AnalysisOptions {
    fn default() -> Self {
        Self {
            native_vocals: cfg!(target_os = "macos"),
            vocal_budget: Duration::from_secs(2),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VocalModelStatus {
    Disabled,
    Unavailable,
    Busy,
    BudgetSkipped,
    Applied,
}

#[derive(Debug)]
pub struct AnalysisReport {
    pub decode_time: Duration,
    pub feature_time: Duration,
    pub vocal_time: Duration,
    pub vocal_status: VocalModelStatus,
}

pub struct Analyzer {
    options: AnalysisOptions,
}

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
    /// Reclassify measured bars without decoding audio or changing the beat grid.
    /// Useful for offline structure audits; persistence remains the caller's job.
    pub fn refresh_structure(analysis: &mut TrackAnalysis) {
        (analysis.sections, analysis.phrase_boundaries) =
            structure::detect(&mut analysis.bars, &analysis.tempo.downbeats);
        analysis.mix_regions = regions::detect(analysis);
    }

    pub fn new() -> Self {
        Self::with_options(AnalysisOptions::default())
    }

    pub fn with_options(options: AnalysisOptions) -> Self {
        Self { options }
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
        self.analyze_track_with_report(id, path)
            .map(|(analysis, _)| analysis)
    }

    /// Device-free timing report for local performance validation. Model failures
    /// leave the DSP analysis usable and never change the measured beat grid.
    pub fn analyze_track_with_report(
        &self,
        id: TrackId,
        path: &Path,
    ) -> Result<(TrackAnalysis, AnalysisReport), AnalyzeError> {
        let start = Instant::now();
        let buf = decode_file(path).map_err(|e| AnalyzeError::Decode(e.to_string()))?;
        let decode_time = start.elapsed();
        let (analysis, mut report) = self.analyze_buffer(id, &buf);
        report.decode_time = decode_time;
        Ok((analysis, report))
    }

    /// Reuse the same decoded samples for features, spectral waveform and playback.
    pub fn analyze_buffer(
        &self,
        id: TrackId,
        buf: &mixless_engine::AudioBuffer,
    ) -> (TrackAnalysis, AnalysisReport) {
        let start = Instant::now();
        let mut analysis = features::analyze(id, &buf.samples, buf.sample_rate);
        let feature_time = start.elapsed();
        let start = Instant::now();
        let vocal_status = sound_analysis::enhance(&mut analysis, &buf.samples, &self.options);
        if vocal_status == VocalModelStatus::Applied {
            (analysis.sections, analysis.phrase_boundaries) =
                structure::detect(&mut analysis.bars, &analysis.tempo.downbeats);
        }
        analysis.mix_regions = regions::detect(&analysis);
        (
            analysis,
            AnalysisReport {
                decode_time: Duration::ZERO,
                feature_time,
                vocal_time: start.elapsed(),
                vocal_status,
            },
        )
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
            phrase_boundaries: vec![],
            mix_regions: vec![],
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
