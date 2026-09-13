//! Native voice evidence supplements (never lowers) the DSP foreground risk.
use crate::{AnalysisOptions, VocalModelStatus};
use mixless_protocol::TrackAnalysis;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct Interval {
    start_sec: f64,
    end_sec: f64,
    confidence: f64,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn mixless_voice_analyze(
        stereo: *const f32,
        frames: usize,
        sr: u32,
        output: *mut Interval,
        capacity: u32,
        budget_seconds: f64,
    ) -> i32;
}

pub(crate) fn enhance(
    analysis: &mut TrackAnalysis,
    stereo: &[f32],
    options: &AnalysisOptions,
) -> VocalModelStatus {
    if !options.native_vocals {
        return VocalModelStatus::Disabled;
    }
    // Do not publish a partial pass when the tail was skipped or time expired.
    if analysis.duration_sec > 600. || analysis.duration_sec < 1.5 || options.vocal_budget.is_zero()
    {
        return VocalModelStatus::BudgetSkipped;
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = stereo;
        VocalModelStatus::Unavailable
    }
    #[cfg(target_os = "macos")]
    {
        static MODEL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let Ok(_guard) = MODEL.try_lock() else {
            return VocalModelStatus::Busy;
        };
        let mut intervals =
            vec![Interval::default(); (analysis.duration_sec / 1.5).ceil() as usize + 2];
        // SAFETY: both slices stay live until the synchronous native call has
        // removed its observer. Writes are bounded by the supplied capacity.
        let count = unsafe {
            mixless_voice_analyze(
                stereo.as_ptr(),
                stereo.len() / 2,
                analysis.sample_rate,
                intervals.as_mut_ptr(),
                intervals.len() as u32,
                options.vocal_budget.as_secs_f64().min(5.),
            )
        };
        match count {
            -2 => VocalModelStatus::BudgetSkipped,
            n if n <= 0 || n as usize > intervals.len() => VocalModelStatus::Unavailable,
            n => {
                intervals.truncate(n as usize);
                if apply(analysis, &intervals) {
                    VocalModelStatus::Applied
                } else {
                    VocalModelStatus::Unavailable
                }
            }
        }
    }
}

fn apply(analysis: &mut TrackAnalysis, intervals: &[Interval]) -> bool {
    if intervals.iter().any(|v| {
        !v.start_sec.is_finite()
            || !v.end_sec.is_finite()
            || !v.confidence.is_finite()
            || v.start_sec < 0.
            || v.end_sec <= v.start_sec
            || v.end_sec > analysis.duration_sec as f64 + 0.1
            || !(0.0..=1.0).contains(&v.confidence)
    }) || intervals
        .windows(2)
        .any(|p| p[1].start_sec < p[0].end_sec - 0.001)
    {
        return false;
    }
    for moment in &mut analysis.moments {
        let center = (moment.start_sec + moment.end_sec) as f64 * 0.5;
        moment.vocal_confidence = intervals
            .iter()
            .find(|v| center >= v.start_sec && center < v.end_sec)
            .map(|v| v.confidence as f32);
    }
    for bar in &mut analysis.bars {
        let mut weighted = 0.;
        let mut coverage = 0.;
        for interval in intervals {
            let overlap = (bar.end_sec as f64).min(interval.end_sec)
                - (bar.start_sec as f64).max(interval.start_sec);
            if overlap > 0. {
                weighted += interval.confidence * overlap;
                coverage += overlap;
            }
        }
        if coverage >= (bar.end_sec - bar.start_sec) as f64 * 0.75 {
            let voice = (weighted / coverage) as f32;
            bar.vocal_confidence = Some(voice);
            bar.vocal_presence = bar.vocal_presence.max(voice);
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track() -> TrackAnalysis {
        crate::features::analyze(
            mixless_protocol::TrackId(1),
            &vec![0.; 22050 * 8 * 2],
            22050,
        )
    }
    #[test]
    fn model_evidence_cannot_clear_a_foreground_and_invalid_results_are_atomic() {
        let mut a = track();
        for b in &mut a.bars {
            b.vocal_presence = 0.8;
        }
        assert!(apply(
            &mut a,
            &[Interval {
                start_sec: 0.,
                end_sec: 8.,
                confidence: 0.05
            }]
        ));
        assert!(a.bars.iter().all(|b| b.vocal_presence == 0.8));
        let before: Vec<_> = a.bars.iter().map(|b| b.vocal_confidence).collect();
        assert!(!apply(
            &mut a,
            &[
                Interval {
                    start_sec: 0.,
                    end_sec: 8.,
                    confidence: 0.9
                },
                Interval {
                    start_sec: 7.,
                    end_sec: 8.,
                    confidence: f64::NAN
                }
            ]
        ));
        assert_eq!(
            before,
            a.bars
                .iter()
                .map(|b| b.vocal_confidence)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn missing_model_coverage_is_unknown_and_budget_skip_preserves_dsp() {
        let mut a = track();
        assert!(apply(
            &mut a,
            &[Interval {
                start_sec: 0.,
                end_sec: 0.1,
                confidence: 0.9
            }]
        ));
        assert!(a.bars.iter().all(|b| b.vocal_confidence.is_none()));
        assert_eq!(
            enhance(
                &mut a,
                &[],
                &AnalysisOptions {
                    native_vocals: true,
                    vocal_budget: std::time::Duration::ZERO
                }
            ),
            VocalModelStatus::BudgetSkipped
        );
    }
    #[test]
    #[cfg(target_os = "macos")]
    #[ignore = "runs the macOS 12+ system model; validate explicitly on a Mac"]
    fn native_model_runs_and_deadline_discards_partial_results() {
        let mut a = track();
        let pcm = vec![0.; 22050 * 8 * 2];
        // Verify actual inference separately from the production two-second
        // budget: a cold model load during compilation can use that budget.
        assert_eq!(
            enhance(
                &mut a,
                &pcm,
                &AnalysisOptions {
                    native_vocals: true,
                    vocal_budget: std::time::Duration::from_secs(5),
                }
            ),
            VocalModelStatus::Applied
        );
        assert!(a.bars.iter().any(|b| b.vocal_confidence.is_some()));
        let mut b = track();
        assert_eq!(
            enhance(
                &mut b,
                &pcm,
                &AnalysisOptions {
                    native_vocals: true,
                    vocal_budget: std::time::Duration::from_nanos(1)
                }
            ),
            VocalModelStatus::BudgetSkipped
        );
        assert!(b.bars.iter().all(|b| b.vocal_confidence.is_none()));
    }
}
