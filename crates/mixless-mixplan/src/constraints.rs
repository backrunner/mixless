use mixless_protocol::{Cue, CueKind};

pub fn covers_user_range(start_sec: f32, end_sec: f32, cues: &[Cue], source_sr: u32) -> bool {
    source_sr > 0
        && start_sec.is_finite()
        && end_sec.is_finite()
        && start_sec <= end_sec
        && cues
            .iter()
            .filter(|c| c.user_set && c.kind != CueKind::Hot)
            .all(|c| {
                let sec = c.frame as f64 / source_sr as f64;
                sec + 0.0001 >= start_sec as f64 && sec <= end_sec as f64 + 0.0001
            })
}

pub(crate) fn user_range(cues: &[Cue], sr: u32) -> Option<(f32, f32)> {
    cues.iter()
        .filter(|c| c.user_set && c.kind != CueKind::Hot)
        .map(|c| c.frame as f32 / sr as f32)
        .fold(None, |range, sec| {
            Some(range.map_or((sec, sec), |(a, b): (f32, f32)| (a.min(sec), b.max(sec))))
        })
}
