use crate::Analyzer;
use mixless_protocol::{StemAnalysis, TrackAnalysis};

impl Analyzer {
    /// Publish complete, source-aligned model evidence. The original mixture
    /// spectrum and grid remain available; notes are not a replacement beat grid.
    pub fn apply_stems(analysis: &mut TrackAnalysis, stems: StemAnalysis) -> bool {
        if !stems.valid(analysis.duration_sec) {
            return false;
        }
        for bar in &mut analysis.bars {
            let first = stems.frames.partition_point(|f| f.end_sec <= bar.start_sec);
            let mut voice = 0.;
            let mut weight = 0.;
            let mut kick = 0.;
            let mut onset = 0.;
            for frame in stems.frames[first..]
                .iter()
                .take_while(|f| f.start_sec < bar.end_sec)
            {
                let w =
                    (frame.end_sec.min(bar.end_sec) - frame.start_sec.max(bar.start_sec)).max(0.);
                voice += frame.vocal_activity * w;
                kick += frame.drum_low_onset * w;
                onset += f32::from(frame.onset[1] > 0.4);
                weight += w;
            }
            if weight >= (bar.end_sec - bar.start_sec) * 0.95 {
                bar.vocal_confidence = Some((voice / weight).clamp(0., 1.));
                // Residual melodic foreground is still protected by note and
                // mixture continuity, independently of the actual vocal lane.
                bar.vocal_presence = (voice / weight).clamp(0., 1.);
                bar.kick_salience = (0.7 * bar.kick_salience
                    + 0.3 * (kick / weight * 5.).clamp(0., 1.))
                .clamp(0., 1.);
                bar.onset_density = 0.7 * bar.onset_density + 0.3 * onset / weight;
            }
        }
        for moment in &mut analysis.moments {
            if let Some(frame) = stems.frame_at((moment.start_sec + moment.end_sec) * 0.5) {
                moment.vocal_confidence = Some(frame.vocal_activity);
            }
        }
        analysis.stems = Some(stems);
        Analyzer::refresh_structure(analysis);
        true
    }
}
