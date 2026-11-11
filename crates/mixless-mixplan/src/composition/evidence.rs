use super::{Fade, PlanContext};
use mixless_protocol::{MixPlan, StemKind, TrackAnalysis};

pub(super) struct Frame {
    pub u: f32,
    pub sec: f32,
    pub bands: [[[f32; 3]; 4]; 2],
    pub voice: [f32; 2],
}

pub(super) fn frames(p: &MixPlan, ctx: &PlanContext<'_>) -> Option<Vec<Frame>> {
    let n = p.summary.as_ref()?.length_bars as f32;
    (0..=128)
        .map(|i| {
            let u = n * i as f32 / 128.;
            let mut bands = [[[0.; 3]; 4]; 2];
            let mut voice = [0f32; 2];
            for (d, t, source) in [
                (0, ctx.outgoing, &p.outgoing_source),
                (1, ctx.incoming, &p.incoming_source),
            ] {
                // A delayed deck has no audible power before its source clock
                // starts. Sampling its parked source position would invent a
                // replacement bed and reward a gap that playback cannot fill.
                if d == 1 && u + 0.0001 < p.incoming_start_bar {
                    continue;
                }
                t.stems.as_ref()?;
                for j in 0..5 {
                    let sec = (source.sample(u) + (j as f32 - 2.) * 0.05)
                        .clamp(0., t.duration_sec - 0.001);
                    let (f, power) = crate::level::evidence::frame(t, sec)?;
                    voice[d] = voice[d].max(f.vocal_activity);
                    for part in 0..3 {
                        for band in 0..3 {
                            // Three measured groups, four playback stems. Low
                            // instruments are a bass proxy, not a fourth meter.
                            let s = if part == 2 && band == 0 { 3 } else { part };
                            bands[d][s][band] += power[part][band] / 5.;
                        }
                    }
                }
            }
            Some(Frame {
                u,
                sec: p.clock.sample(u),
                bands,
                voice,
            })
        })
        .collect()
}

/// Fade only while the singer is resting. A low frame between consonants or
/// a sustained note at a section edge cannot authorize removing a phrase.
pub(super) fn voice_windows(
    source: &mixless_protocol::Polyline,
    t: &TrackAnalysis,
    n: f32,
    minimum_bars: f32,
) -> Vec<Fade> {
    let Some(meters) = &t.stems else {
        return vec![];
    };
    let mut windows = vec![];
    // A corroborated repeating sample can fade as a rhythmic part while the
    // same control would truncate a sentence. Require local evidence across
    // the entire fade; final heard harmony and energy checks still apply.
    for i in 0..=(n * 4.) as usize {
        let u = i as f32 / 4.;
        let end = u + minimum_bars.max(0.25);
        if end <= n && crate::vocals::chops(t, source.sample(u), source.sample(end)) {
            windows.push(Fade { start: u, end });
        }
    }
    // A transcribed note in separator leakage is not a sung foreground.
    // Use a relative -40 dB power floor, invariant to recording gain.
    let negligible = |f: &mixless_protocol::StemFrame| {
        f.rms[0].powi(2) <= (f.rms[1].powi(2) + f.rms[2].powi(2)) * 0.0001
    };
    // Include exact measured quiet-frame starts so a short real breath is
    // not lost between subdivisions of the planner grid.
    for frame in meters.frames.iter().filter(|f| {
        (f.vocal_activity < 0.2 || negligible(f))
            && f.start_sec >= source.sample(0.)
            && f.start_sec < source.sample(n)
    }) {
        let u = super::bar_at(source, frame.start_sec);
        let (lo, hi) = ((u - 0.5).max(0.), (u + 0.5).min(n));
        let seconds_per_bar = (source.sample(hi) - source.sample(lo)) / (hi - lo).max(0.001);
        let width = minimum_bars.max(0.2 / seconds_per_bar.max(0.01));
        if u + width > n {
            continue;
        }
        let (start, end) = (source.sample(u), source.sample(u + width));
        if crate::vocals::continues(t, (start + end) * 0.5) {
            continue;
        }
        let first = meters.frames.partition_point(|f| f.end_sec <= start);
        let last = meters.frames.partition_point(|f| f.start_sec < end);
        let fs = &meters.frames[first..last];
        let covered: f32 = fs
            .iter()
            .map(|f| (f.end_sec.min(end) - f.start_sec.max(start)).max(0.))
            .sum();
        if covered < (end - start) * 0.95 || fs.is_empty() {
            continue;
        }
        if fs.iter().any(|f| f.vocal_activity >= 0.2 && !negligible(f))
            || (!fs.iter().all(|f| negligible(f))
                && meters.notes.iter().any(|note| {
                    note.stem == StemKind::Vocals
                        && note.confidence >= 0.45
                        && note.start_sec < end
                        && note.end_sec > start
                        && crate::vocals::note_supported(t, note)
                }))
        {
            continue;
        }
        windows.push(Fade {
            start: u,
            end: u + width,
        });
    }
    windows.sort_by(|a, b| a.start.total_cmp(&b.start));
    windows
}
