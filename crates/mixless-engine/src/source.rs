//! Source-domain stem mixing shares interpolation, transport and one stretcher.
use crate::AudioBuffer;

/// Aligned, pre-touched PCM, prepared on a host worker. Instruments include the
/// separation residual: original - vocals - drums. Unity is the exact original.
#[derive(Debug)]
pub struct StemBuffer {
    pub(crate) vocals: Vec<f32>,
    pub(crate) drums: Vec<f32>,
    pub(crate) frames: u64,
    pub(crate) sample_rate: u32,
}
impl StemBuffer {
    pub fn new(sample_rate: u32, vocals: Vec<f32>, drums: Vec<f32>) -> Result<Self, &'static str> {
        if sample_rate == 0
            || vocals.is_empty()
            || vocals.len() % 2 != 0
            || vocals.len() != drums.len()
            || vocals.iter().chain(&drums).any(|v| !v.is_finite())
        {
            return Err("Invalid aligned stem PCM");
        }
        Ok(Self {
            frames: (vocals.len() / 2) as u64,
            sample_rate,
            vocals,
            drums,
        })
    }
    pub fn bytes(&self) -> usize {
        (self.vocals.len() + self.drums.len()) * 4
    }
}

pub(crate) trait SampleSource {
    fn frames(&self) -> u64;
    fn sample(&self, index: usize) -> [f32; 2];
    fn contiguous(&self) -> Option<&[f32]> {
        None
    }
    fn stereo_at(&self, frame: f64) -> (f32, f32) {
        if !frame.is_finite()
            || self.frames() == 0
            || frame < 0.
            || frame > self.frames().saturating_sub(1) as f64
        {
            return (0., 0.);
        }
        let i = frame.floor() as usize;
        let t = (frame - i as f64) as f32;
        let last = self.frames() as usize - 1;
        if t == 0. {
            let s = self.sample(i);
            return (s[0], s[1]);
        }
        let a = self.sample(i.saturating_sub(1));
        let b = self.sample(i);
        let c = self.sample((i + 1).min(last));
        let d = self.sample((i + 2).min(last));
        let interpolate = |ch: usize| {
            let slope = (c[ch] - a[ch]) * 0.5;
            let quadratic = a[ch] - b[ch] * 2.5 + c[ch] * 2. - d[ch] * 0.5;
            let cubic = (d[ch] - a[ch]) * 0.5 + (b[ch] - c[ch]) * 1.5;
            ((cubic * t + quadratic) * t + slope) * t + b[ch]
        };
        (interpolate(0), interpolate(1))
    }
}
impl SampleSource for AudioBuffer {
    #[inline]
    fn frames(&self) -> u64 {
        self.frames
    }
    #[inline]
    fn sample(&self, i: usize) -> [f32; 2] {
        [self.samples[i * 2], self.samples[i * 2 + 1]]
    }
    fn contiguous(&self) -> Option<&[f32]> {
        Some(&self.samples)
    }
    #[inline]
    fn stereo_at(&self, frame: f64) -> (f32, f32) {
        AudioBuffer::stereo_at(self, frame)
    }
}
impl<T: SampleSource> SampleSource for std::sync::Arc<T> {
    fn frames(&self) -> u64 {
        (**self).frames()
    }
    fn sample(&self, i: usize) -> [f32; 2] {
        (**self).sample(i)
    }
    fn contiguous(&self) -> Option<&[f32]> {
        (**self).contiguous()
    }
    fn stereo_at(&self, f: f64) -> (f32, f32) {
        (**self).stereo_at(f)
    }
}

pub(crate) struct StemSource<'a> {
    original: &'a AudioBuffer,
    stems: Option<&'a StemBuffer>,
    coefficients: [f32; 3],
}
impl<'a> StemSource<'a> {
    pub fn new(original: &'a AudioBuffer, stems: Option<&'a StemBuffer>, gains: [f32; 3]) -> Self {
        let coefficients = if stems.is_some() {
            [gains[2], gains[0] - gains[2], gains[1] - gains[2]]
        } else {
            [1., 0., 0.]
        };
        Self {
            original,
            stems,
            coefficients,
        }
    }
}
impl SampleSource for StemSource<'_> {
    #[inline]
    fn frames(&self) -> u64 {
        self.original.frames
    }
    #[inline]
    fn sample(&self, i: usize) -> [f32; 2] {
        let [mix, vocal, drum] = self.coefficients;
        let mut s = [
            self.original.samples[i * 2] * mix,
            self.original.samples[i * 2 + 1] * mix,
        ];
        if let Some(stems) = self.stems {
            if vocal != 0. {
                s[0] += stems.vocals[i * 2] * vocal;
                s[1] += stems.vocals[i * 2 + 1] * vocal;
            }
            if drum != 0. {
                s[0] += stems.drums[i * 2] * drum;
                s[1] += stems.drums[i * 2 + 1] * drum;
            }
        }
        s
    }
    fn contiguous(&self) -> Option<&[f32]> {
        (self.coefficients == [1., 0., 0.]).then_some(&self.original.samples)
    }
}
