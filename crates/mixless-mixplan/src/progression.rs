//! Compare sounding pitch-class motion through the aligned overlap. This is
//! native note evidence when available, with mixture-spectrum fallback.
use mixless_protocol::{Polyline, TrackAnalysis};

fn pitch(t: &TrackAnalysis, sec: f32) -> Option<([f32; 12], f32, f32)> {
    if let Some(f) = t.stems.as_ref().and_then(|s| s.frame_at(sec)) {
        if f.note_chroma.iter().any(|v| *v > 0.1) {
            return Some((f.note_chroma, f.rms[0] + f.rms[2], 0.9));
        }
    }
    crate::continuity::moment(t, sec).map(|m| (m.chroma, m.rms, m.sustain))
}

pub(crate) fn tension(
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    sa: &Polyline,
    sb: &Polyline,
    n: f32,
    shift_a: f32,
    shift_b: f32,
) -> f32 {
    let mut total = 0.;
    let mut weight = 0.;
    let mut run = 0f32;
    let mut longest = 0f32;
    // Sample all of the musical interval, bounded independently of FFT count.
    for i in 0..128 {
        let u = n * (i as f32 + 0.5) / 128.;
        let (Some(a), Some(b)) = (pitch(a, sa.sample(u)), pitch(b, sb.sample(u))) else {
            continue;
        };
        let w = a.2.min(b.2) * (std::f32::consts::PI * u / n.max(0.01)).sin().max(0.);
        if a.1 < 0.001 || b.1 < 0.001 || w < 0.15 {
            run = 0.;
            continue;
        }
        let mut cost = 0.;
        let mut mass = 0.;
        for (pa, va) in a.0.iter().enumerate() {
            for (pb, vb) in b.0.iter().enumerate() {
                let interval = ((pa as i32 + shift_a.round() as i32)
                    - (pb as i32 + shift_b.round() as i32))
                    .rem_euclid(12) as usize;
                let dissonance = [
                    0., 1., 0.65, 0.12, 0.1, 0.2, 0.85, 0.05, 0.1, 0.12, 0.65, 1.,
                ][interval];
                let evidence = va.max(0.).powi(2) * vb.max(0.).powi(2);
                cost += dissonance * evidence;
                mass += evidence;
            }
        }
        if mass < 0.01 {
            continue;
        }
        cost /= mass;
        total += cost * w;
        weight += w;
        if cost > 0.6 {
            run += 1. / 128.;
            longest = longest.max(run);
        } else {
            run = 0.;
        }
    }
    if weight < 1. {
        0.
    } else {
        (total / weight * 0.75 + longest * 0.25).clamp(0., 1.)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_aligned_motion_and_applied_pitch_without_inventing_missing_notes() {
        let mut a = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            16,
            0.8,
            0.2,
        );
        let mut b = a.clone();
        for i in 0..200 {
            let m = mixless_protocol::MusicalMoment {
                start_sec: i as f32 * 0.05,
                end_sec: (i + 1) as f32 * 0.05,
                rms: 0.2,
                sustain: 0.9,
                chroma: std::array::from_fn(|pc| if pc == (i / 50) % 12 { 1. } else { 0. }),
                band_db: [-18.; 3],
                onset: 0.,
                attack_sec: 0.,
                low_onset: 0.,
                pitch_midi: None,
                vocal_confidence: Some(0.),
            };
            a.moments.push(m.clone());
            b.moments.push(m);
        }
        let source = Polyline {
            nodes: vec![(0., 0.), (8., 10.)],
        };
        assert!(tension(&a, &b, &source, &source, 8., 0., 0.) < 0.01);
        assert!(tension(&a, &b, &source, &source, 8., 0., 1.) > 0.7);
        assert!(tension(&a, &b, &source, &source, 8., 1., 1.) < 0.01);
        b.moments.clear();
        assert_eq!(tension(&a, &b, &source, &source, 8., 0., 1.), 0.);
    }
}
