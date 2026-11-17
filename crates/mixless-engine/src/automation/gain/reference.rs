//! Local endpoint matching without flattening either recording's dynamics.
use super::*;

fn median(values: impl Iterator<Item = f64>) -> Option<f64> {
    let mut values: Vec<_> = values.filter(|v| v.is_finite() && *v > 1e-6).collect();
    if values.len() < 4 {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

pub(super) fn shape(dense: &DensePlan, preview: &mut render::Preview, dt: f32) {
    let duration = dense.overlap_frames as f32 / dense.sample_rate as f32;
    let start = dense.incoming_start_frames as f32 / dense.sample_rate as f32;
    let end = (dense.overlap_frames / STEP).min(dense.points.len() - 1);
    // An intentional partial crossfader landing is not a full foreground
    // exchange. Do not reinterpret those routes as two solo anchors.
    if dense.points[0].xf > 10 || dense.points[end].xf < 990 || duration - start < 2. {
        return;
    }
    let count = (duration / dt).ceil() as usize;
    let Some(a) = median(
        preview
            .history
            .iter()
            .rev()
            .take((2. / dt) as usize)
            .map(|r| r[0].power[0]),
    ) else {
        return;
    };
    let Some(b) = median(
        preview
            .readings
            .iter()
            .skip(count + (1. / dt) as usize)
            .map(|r| r[1].power[0]),
    ) else {
        return;
    };
    if (10. * (a / b).log10()).abs() <= 1. {
        return;
    }
    let anchors = [a, b];
    let original = preview.readings.clone();
    for (i, reading) in preview.readings.iter_mut().take(count).enumerate() {
        let x = ((i as f32 * dt - start) / (duration - start)).clamp(0., 1.) as f64;
        let progress = x * x * (3. - 2. * x);
        let level = (a.ln() * (1. - progress) + b.ln() * progress).exp();
        for d in 0..2 {
            // The scalar is slow; the source's measured local dynamics and
            // each band ratio remain intact, including real gaps and builds.
            let ratio = level / anchors[d];
            for power in &mut reading[d].power {
                *power *= ratio;
            }
        }
    }
    // Project the slow target into the same three-second context used by the
    // short-term guard. This is a policy target; measured LUFS stays untouched.
    let width = (3. / dt).ceil() as usize;
    for i in 0..preview.readings.len() {
        for d in 0..2 {
            let range = i.saturating_sub(width - 1)..=i;
            let before: f64 = range.clone().map(|j| original[j][d].power[0]).sum();
            let after: f64 = range.map(|j| preview.readings[j][d].power[0]).sum();
            if before > 1e-9 {
                preview.readings[i][d].short = original[i][d].short * after / before;
            }
        }
    }
}
