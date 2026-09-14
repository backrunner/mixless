//! Fine spectral continuity supplements bar/section boundaries. A bar line by
//! itself is not evidence that a held note or a sung syllable has finished.
use mixless_protocol::{MusicalMoment, TrackAnalysis};

pub(crate) fn voice_gap(t: &TrackAnalysis, sec: f32) -> Option<bool> {
    let stems = t.stems.as_ref()?;
    let first = stems.frames.partition_point(|f| f.end_sec <= sec - 0.1);
    let last = stems.frames.partition_point(|f| f.start_sec < sec + 0.1);
    let frames = &stems.frames[first..last];
    (frames.len() >= 3).then(|| {
        frames
            .iter()
            .all(|f| f.vocal_activity < 0.2 || f.rms[0] < 0.0015)
    })
}

pub(crate) fn moment(t: &TrackAnalysis, sec: f32) -> Option<&MusicalMoment> {
    let i = t.moments.partition_point(|m| m.end_sec <= sec);
    t.moments.get(i).filter(|m| m.start_sec <= sec)
}

pub(crate) fn gap(t: &TrackAnalysis, sec: f32) -> bool {
    let first = t.moments.partition_point(|m| m.end_sec < sec - 0.12);
    let last = t.moments.partition_point(|m| m.start_sec <= sec + 0.08);
    let center = &t.moments[first..last];
    if center.len() < 3 {
        return false;
    }
    let peak = t.moments[t.moments.partition_point(|m| m.end_sec < sec - 0.7)
        ..t.moments.partition_point(|m| m.start_sec <= sec + 0.4)]
        .iter()
        .map(|m| m.band_db[1])
        .fold(-120., f32::max);
    // Require a sustained trough, not one FFT frame between consonants. The
    // native classifier's broad interval cannot locate a breath on its own.
    center
        .iter()
        .all(|m| m.rms < 0.001 || m.band_db[1] < peak - 12.)
}

pub(crate) fn voice_active(t: &TrackAnalysis, sec: f32) -> f32 {
    if let Some(frame) = t.stems.as_ref().and_then(|s| s.frame_at(sec)) {
        return frame.vocal_activity;
    }
    moment(t, sec).map_or_else(
        || crate::musical::feature(t, sec).map_or(1., crate::vocals::risk),
        |m| {
            if m.rms <= 0.001 {
                0.
            } else {
                m.vocal_confidence.unwrap_or(m.sustain)
            }
        },
    )
}

pub(crate) fn voice_cut_safe(t: &TrackAnalysis, sec: f32) -> bool {
    if sec <= 0.001 || sec >= t.duration_sec - 0.001 {
        return true;
    }
    if let Some(stems) = &t.stems {
        if stems.note_crossing(sec, mixless_protocol::StemKind::Vocals)
            && voice_active(t, sec) > 0.2
        {
            return false;
        }
        // A new note at a measured arrangement change can be left to the next
        // track; it is distinct from severing a note already in progress. It
        // authorizes the cut when the previous phrase actually released —
        // either the voice was already quiet, or a sung note ended right at
        // the boundary and nothing sounds just after it. An onset with the
        // voice still carrying through is a new syllable, not a release.
        let structural = t
            .sections
            .iter()
            .any(|s| (s.start_sec - sec).abs() < 0.04 || (s.end_sec - sec).abs() < 0.04);
        let released = (voice_active(t, sec - 0.15) < 0.35 && voice_active(t, sec - 0.30) < 0.5)
            || (stems.notes.iter().any(|n| {
                n.stem == mixless_protocol::StemKind::Vocals
                    && n.confidence >= 0.45
                    && n.end_sec > sec - 0.30
                    && n.end_sec < sec + 0.02
            }) && voice_active(t, sec + 0.10) < 0.4);
        if structural
            && released
            && stems.notes.iter().any(|n| {
                n.stem == mixless_protocol::StemKind::Vocals
                    && (n.start_sec - sec).abs() < 0.035
                    && n.confidence >= 0.5
            })
        {
            return true;
        }
        if let Some(gap) = voice_gap(t, sec) {
            return gap || voice_active(t, sec - 0.08) < 0.35 || voice_active(t, sec + 0.08) < 0.25;
        }
    }
    if t.moments.is_empty() {
        return !crate::vocals::phrases(t)
            .iter()
            .any(|&(a, b)| sec > a + 0.05 && sec < b - 0.05);
    }
    if gap(t, sec) {
        return true;
    }
    voice_active(t, sec - 0.08) < 0.45 || voice_active(t, sec + 0.08) < 0.35
}

/// The outgoing voice must have released by the time a hard cut or FX bridge
/// lands — but it may have sung right up to the boundary. A build whose vocal
/// stops on the downbeat is a legitimate cut point; what is not legitimate is
/// a phrase still sounding past `sec`. Released means a measured breath or
/// moment gap at `sec`, a sung note ending on the boundary with no vocal
/// event continuing past it, or low activity in the first ~0.4 s after the
/// cut. With no detailed evidence the coarse phrase map decides.
pub(crate) fn voice_released(t: &TrackAnalysis, sec: f32) -> bool {
    if t.stems.is_none() && t.moments.is_empty() {
        return !crate::vocals::phrases(t)
            .iter()
            .any(|&(a, b)| sec > a + 0.05 && sec < b - 0.05);
    }
    if voice_gap(t, sec) == Some(true) || gap(t, sec) {
        return true;
    }
    if let Some(stems) = &t.stems {
        let ended = stems.notes.iter().any(|n| {
            n.stem == mixless_protocol::StemKind::Vocals
                && n.confidence >= 0.45
                && n.end_sec > sec - 0.30
                && n.end_sec < sec + 0.12
        });
        let continues = stems.notes.iter().any(|n| {
            n.stem == mixless_protocol::StemKind::Vocals
                && n.confidence >= 0.45
                && n.start_sec < sec + 0.15
                && n.end_sec > sec + 0.30
        });
        if ended && !continues {
            return true;
        }
    }
    let mut mean = 0.;
    let mut count = 0usize;
    let mut s = sec + 0.05;
    while s < sec + 0.45 && s < t.duration_sec {
        mean += voice_active(t, s);
        count += 1;
        s += 0.05;
    }
    count == 0 || mean / (count as f32) < 0.35
}

pub(crate) fn cut_safe(t: &TrackAnalysis, sec: f32, drop_boundary: bool) -> bool {
    if !voice_cut_safe(t, sec) {
        return false;
    }
    if !drop_boundary
        && t.stems.as_ref().is_some_and(|s| {
            s.note_crossing(sec, mixless_protocol::StemKind::Instruments)
                && s.frame_at(sec).is_some_and(|f| f.rms[2] > 0.003)
        })
    {
        return false;
    }
    if gap(t, sec) {
        return true;
    }
    let (Some(before), Some(after)) = (moment(t, sec - 0.08), moment(t, sec + 0.08)) else {
        return crate::phrasing::boundary_quality(t, sec) >= 0.6;
    };
    // A measured build/drop boundary deliberately resolves a riser. Ordinary
    // inferred phrase markers cannot interrupt a continuous pitched foreground.
    if drop_boundary {
        return true;
    }
    let structural = t
        .sections
        .iter()
        .any(|s| (s.start_sec - sec).abs() < 0.08 || (s.end_sec - sec).abs() < 0.08)
        || t.phrase_boundaries
            .iter()
            .any(|p| (p.time_sec - sec).abs() < 0.08 && p.novelty >= 0.5);
    // A moving melody may change pitch precisely on a bar line. Inspect its
    // continuity over the neighboring beats instead of mistaking that new note
    // for the end of the musical idea. Only measured arrangement changes can
    // authorize a cut there; a long level/EQ release remains available.
    if !structural {
        let span = 120. / t.tempo.global_bpm.max(30.);
        let pitched = |start: f32, end: f32| {
            let first = t.moments.partition_point(|m| m.end_sec <= start);
            let last = t.moments.partition_point(|m| m.start_sec < end);
            let frames = &t.moments[first..last];
            !frames.is_empty()
                && frames
                    .iter()
                    .filter(|m| m.rms > 0.001 && m.pitch_midi.is_some() && m.sustain > 0.35)
                    .count() as f32
                    / frames.len() as f32
                    > 0.65
        };
        if pitched(sec - span, sec) && pitched(sec, sec + span) {
            return false;
        }
    }
    let chroma = before
        .chroma
        .iter()
        .zip(after.chroma)
        .map(|(a, b)| a * b)
        .sum::<f32>();
    let held_pitch = before
        .pitch_midi
        .zip(after.pitch_midi)
        .is_some_and(|(a, b)| (a - b).abs() < 0.65);
    !(before.sustain > 0.45
        && after.sustain > 0.45
        && (chroma > 0.85 || held_pitch)
        && after.rms > before.rms * 0.5)
}

/// Choose a release from the actual phrase/gap evidence in the overlap.
/// Without a breath, use the entire interval for a gradual foreground release.
pub(crate) fn fade_start(t: &TrackAnalysis, start: f32, end: f32) -> f32 {
    if let Some(stems) = &t.stems {
        return stems
            .frames
            .iter()
            .filter(|f| f.start_sec >= start && f.start_sec < start + (end - start) * 0.5)
            .find(|f| voice_gap(t, f.start_sec) == Some(true))
            .map_or(start, |f| f.start_sec);
    }
    let first = t.moments.partition_point(|m| m.start_sec < start);
    t.moments[first..]
        .iter()
        .take_while(|m| m.start_sec < start + (end - start) * 0.5)
        .find(|m| gap(t, m.start_sec))
        .map_or(start, |m| m.start_sec)
}
