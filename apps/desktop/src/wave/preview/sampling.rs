//! Match thumbnail detail to physical pixels, with bounded resize requests.
use super::cache::Column;

pub(super) fn resolution(width: f32, scale: f32) -> usize {
    // Small buckets avoid rebuilding for every point during a window drag.
    // The cap bounds each cached row even on very large displays.
    let pixels = (width * scale).ceil().clamp(256., 8192.) as usize;
    pixels.div_ceil(256) * 256
}

pub(super) fn sample(wave: &[Column], center: f64, columns_per_pixel: f32) -> Column {
    if columns_per_pixel <= 1. {
        let x = (center - 0.5).clamp(0., (wave.len() - 1) as f64);
        let i = x.floor() as usize;
        return wave[i].blend(wave[(i + 1).min(wave.len() - 1)], (x - i as f64) as f32);
    }
    // A high-resolution cached preview can be reused when the window shrinks.
    // Reduce the entire pixel footprint so narrow transients cannot fall
    // between point samples; average color by coverage, retain envelope peaks.
    let half = columns_per_pixel as f64 * 0.5;
    let start = (center - half).max(0.);
    let end = (center + half).min(wave.len() as f64);
    let first = (start.floor() as usize).min(wave.len() - 1);
    let last = (end.ceil() as usize).min(wave.len());
    let mut result = wave[first];
    let mut weight = 0.;
    let mut pos = 0_f32;
    let mut neg = 0_f32;
    for (i, column) in wave.iter().enumerate().take(last).skip(first) {
        let coverage = (end.min((i + 1) as f64) - start.max(i as f64)) as f32;
        if coverage <= 0. {
            continue;
        }
        weight += coverage;
        result = result.blend(*column, coverage / weight);
        pos = pos.max(column.pos);
        neg = neg.max(column.neg);
    }
    result.pos = pos;
    result.neg = neg;
    result.peak = pos.max(neg);
    result
}
