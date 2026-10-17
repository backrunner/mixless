use crate::Analyzer;
use mixless_protocol::{StemAnalysis, TrackAnalysis};

impl Analyzer {
    /// Publish complete, source-aligned model evidence. The original mixture
    /// spectrum and grid remain available; notes are not a replacement beat grid.
    pub fn apply_stems(analysis: &mut TrackAnalysis, stems: StemAnalysis) -> bool {
        if !stems.valid(analysis.duration_sec) {
            return false;
        }
        // The separated drum level per bar. A mixture bass line can read as a
        // kick; when the drum stem itself has nearly left, the bar has no beat.
        let drum_level = |start: f32, end: f32| {
            let first = stems.frames.partition_point(|f| f.end_sec <= start);
            let frames: Vec<f32> = stems.frames[first..]
                .iter()
                .take_while(|f| f.start_sec < end)
                .map(|f| f.rms[1])
                .collect();
            frames.iter().sum::<f32>() / frames.len().max(1) as f32
        };
        let drums: Vec<f32> = analysis
            .bars
            .iter()
            .map(|bar| drum_level(bar.start_sec, bar.end_sec))
            .collect();
        let mut sorted: Vec<f32> = drums.iter().copied().filter(|d| d.is_finite()).collect();
        sorted.sort_by(f32::total_cmp);
        let reference = sorted
            .get((sorted.len().saturating_sub(1)) * 4 / 5)
            .copied()
            .unwrap_or(0.);
        for (bar, drum) in analysis.bars.iter_mut().zip(&drums) {
            // Below a third of the track's usual kit the beat fades out with it.
            let presence = if reference > 0.001 {
                (drum / (reference * 0.33)).clamp(0., 1.)
            } else {
                1.
            };
            bar.kick_salience *= presence;
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
