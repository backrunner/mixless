//! Multiresolution source envelopes. Resampling blends adjacent bins and levels.
use super::*;

#[derive(Clone, Copy)]
pub(super) struct Column {
    pub peak: f32,
    pub pos: f32,
    pub neg: f32,
    pub rms: f32,
    pub color: Rgba,
}
impl Column {
    pub(super) fn blend(self, other: Self, t: f32) -> Self {
        let lerp = |a: f32, b: f32| a + (b - a) * t;
        Self {
            peak: lerp(self.peak, other.peak),
            pos: lerp(self.pos, other.pos),
            neg: lerp(self.neg, other.neg),
            rms: lerp(self.rms, other.rms),
            color: Rgba {
                r: lerp(self.color.r, other.color.r),
                g: lerp(self.color.g, other.color.g),
                b: lerp(self.color.b, other.color.b),
                a: 1.,
            },
        }
    }
}
pub struct WaveCache {
    pub source: Arc<Waveform>,
    pub(super) columns: usize,
    levels: Vec<Vec<Column>>,
    pub(super) tiles: tiles::TileCache,
}
#[derive(Clone, Copy)]
struct Energy {
    pos: f32,
    neg: f32,
    rms: f32,
    bands: [f32; 4],
    count: f32,
}
impl Energy {
    fn paint(self) -> Column {
        let pos = self.pos.powf(0.85);
        let neg = self.neg.powf(0.85);
        Column {
            peak: pos.max(neg),
            pos,
            neg,
            rms: (self.rms / self.count).sqrt().powf(0.85),
            color: spectrum(self.bands),
        }
    }
    fn merge(self, other: Self) -> Self {
        Self {
            pos: self.pos.max(other.pos),
            neg: self.neg.max(other.neg),
            rms: self.rms + other.rms,
            bands: std::array::from_fn(|i| self.bands[i] + other.bands[i]),
            count: self.count + other.count,
        }
    }
}
impl WaveCache {
    pub fn new(source: Arc<Waveform>) -> Self {
        let n = (source.columns as usize)
            .min(source.peak.len())
            .min(source.low.len())
            .min(source.mid.len())
            .min(source.high.len());
        let mut energy: Vec<_> = (0..n)
            .map(|i| {
                let value = |detail: &[u16], coarse: &[u8]| {
                    detail.get(i).map_or_else(
                        || coarse.get(i).copied().unwrap_or(source.peak[i]) as f32 / 255.,
                        |v| *v as f32 / 65535.,
                    )
                };
                let pos = value(&source.detail_pos, &source.peak_pos);
                let neg = value(&source.detail_neg, &source.peak_neg);
                let rms = value(&source.detail_rms, &source.rms);
                Energy {
                    pos,
                    neg,
                    rms: rms * rms,
                    bands: [
                        source.low[i],
                        source.low_mid.get(i).copied().unwrap_or(0),
                        source.mid[i],
                        source.high[i],
                    ]
                    .map(|v| v as f32 * rms * rms),
                    count: 1.,
                }
            })
            .collect();
        let mut levels = Vec::new();
        while !energy.is_empty() {
            levels.push(energy.iter().map(|e| e.paint()).collect());
            if energy.len() == 1 {
                break;
            }
            energy = energy
                .chunks(2)
                .map(|c| if c.len() == 2 { c[0].merge(c[1]) } else { c[0] })
                .collect();
        }
        Self {
            source,
            columns: n,
            levels,
            tiles: Default::default(),
        }
    }
    pub fn poll_tiles(&self) -> bool {
        self.tiles.poll()
    }
    fn sample_level(&self, source_column: f64, level: usize) -> Column {
        let columns = &self.levels[level];
        let x =
            (source_column / (1usize << level) as f64 - 0.5).clamp(0., (columns.len() - 1) as f64);
        let i = x.floor() as usize;
        columns[i].blend(
            columns[(i + 1).min(columns.len() - 1)],
            (x - i as f64) as f32,
        )
    }
    pub(super) fn sample(&self, source_column: f64, columns_per_pixel: f32) -> Column {
        let lod = columns_per_pixel
            .max(1.)
            .log2()
            .clamp(0., (self.levels.len() - 1) as f32);
        let level = lod.floor() as usize;
        let a = self.sample_level(source_column, level);
        if level + 1 < self.levels.len() {
            a.blend(
                self.sample_level(source_column, level + 1),
                lod - level as f32,
            )
        } else {
            a
        }
    }
}
/// Compact exactly the same positive/negative/RMS and spectral evidence used
/// by the scrolling pyramid, without retaining a large source payload per row.
pub(super) fn overview(source: &Waveform, columns: usize) -> Vec<Column> {
    let count = (source.columns as usize).min(source.peak.len());
    let n = columns.min(count);
    let mut result = Vec::with_capacity(n);
    for c in 0..n {
        let mut e = Energy {
            pos: 0.,
            neg: 0.,
            rms: 0.,
            bands: [0.; 4],
            count: 0.,
        };
        for i in c * count / n..(c + 1) * count / n {
            let value = |detail: &[u16], coarse: &[u8]| {
                detail.get(i).map_or_else(
                    || coarse.get(i).copied().unwrap_or(source.peak[i]) as f32 / 255.,
                    |v| *v as f32 / 65535.,
                )
            };
            let rms = value(&source.detail_rms, &source.rms);
            let power = rms * rms;
            let bands = [&source.low, &source.low_mid, &source.mid, &source.high]
                .map(|b| b.get(i).copied().unwrap_or(0) as f32 * power);
            e = e.merge(Energy {
                pos: value(&source.detail_pos, &source.peak_pos),
                neg: value(&source.detail_neg, &source.peak_neg),
                rms: power,
                bands,
                count: 1.,
            });
        }
        result.push(e.paint());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pyramid_preserves_transients_tail_and_continuous_subpixel_motion() {
        let wave = Arc::new(Waveform {
            columns: 5,
            duration_sec: 1.,
            peak: vec![0, 255, 0, 0, 128],
            peak_pos: vec![],
            peak_neg: vec![],
            rms: vec![0, 100, 0, 0, 50],
            low: vec![255; 5],
            low_mid: vec![0; 5],
            mid: vec![0; 5],
            high: vec![0; 5],
            detail_pos: vec![],
            detail_neg: vec![],
            detail_rms: vec![],
        });
        let compact = overview(&wave, 5);
        let reduced = overview(&wave, 2);
        let cache = WaveCache::new(wave);
        for (a, b) in compact.iter().zip(&cache.levels[0]) {
            assert_eq!(a.color, b.color);
            assert_eq!(a.pos, b.pos);
            assert_eq!(a.rms, b.rms);
        }
        assert_eq!(reduced[0].peak, 1.);
        assert!(reduced[1].peak > 0.5);
        assert_eq!(cache.levels[1][0].peak, 1.);
        assert!(cache.levels[1][2].peak > 0.5);
        for x in 0..400 {
            let p = x as f64 / 100.;
            assert!((cache.sample(p, 2.5).peak - cache.sample(p + 0.01, 2.5).peak).abs() < 0.02);
        }
        assert!((cache.sample(1.3, 1.9999).peak - cache.sample(1.3, 2.0001).peak).abs() < 0.001);
    }
}
