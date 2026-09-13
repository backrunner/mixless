use mixless_protocol::{SectionLabel as S, TrackAnalysis};

/// Once a build has been heard, resolve it on its actual final downbeat. A
/// layered build is possible only when the incoming drop resolves there too.
pub(crate) fn breaks_build(a: &TrackAnalysis, end: f32, b: &TrackAnalysis, incoming: f32) -> bool {
    a.sections
        .iter()
        .filter(|s| {
            s.label == S::BuildUp && mixless_protocol::has_buildup(&a.bars, s.start_sec, s.end_sec)
        })
        .any(|s| {
            end > s.start_sec + 0.08
                && end <= s.end_sec + 0.08
                && ((end - s.end_sec).abs() > 0.08
                    || !b.sections.iter().any(|next| {
                        next.label == S::Drop && (next.start_sec - incoming).abs() < 0.08
                    }))
        })
}
