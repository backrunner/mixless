//! Compare the actual aligned overlap, independently for bass, mids and highs.
use mixless_protocol::{Polyline, TrackAnalysis};

pub(crate) struct BandMatch {
    pub mid_cut_db: f32,
    pub high_cut_db: f32,
    pub highpass_hz: f32,
}

pub(crate) fn compare(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    sa: &Polyline,
    sb: &Polyline,
    n: f32,
) -> BandMatch {
    let mut count = 0f32;
    let mut busy_high = 0.;
    let mut voice = 0f32;
    let mut bass_conflict = 0.;
    for i in 0..64 {
        let u = n * (i as f32 + 0.5) / 64.;
        let (Some(fa), Some(fb)) = (
            crate::musical::feature(a, sa.sample(u)),
            crate::musical::feature(b, sb.sample(u)),
        ) else {
            continue;
        };
        count += 1.;
        voice = voice.max(
            crate::continuity::voice_active(a, sa.sample(u))
                .min(crate::continuity::voice_active(b, sb.sample(u))),
        );
        busy_high += ((fb.high_db - fa.high_db + 12.) / 24.).clamp(0., 1.);
        bass_conflict += match (
            a.stems.as_ref().and_then(|s| s.frame_at(sa.sample(u))),
            b.stems.as_ref().and_then(|s| s.frame_at(sb.sample(u))),
        ) {
            (Some(a), Some(b)) => ((a.band_db[1][0] - b.band_db[1][0]).abs() / 18.).clamp(0., 1.),
            _ => (fa.kick_salience - fb.kick_salience).abs(),
        };
    }
    let count = count.max(1.);
    BandMatch {
        mid_cut_db: 24. + 12. * voice,
        high_cut_db: (busy_high / count * 8. + voice * 10.).clamp(3., 18.),
        highpass_hz: (900. + bass_conflict / count * 900. + voice * 700.).clamp(900., 2500.),
    }
}
