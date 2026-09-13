use std::fs::File;
use std::path::Path;
use std::sync::Arc;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("decode: {0}")]
    Sym(String),
    #[error("no audio track")]
    NoTrack,
    #[error("empty decode")]
    Empty,
}

impl From<SymError> for DecodeError {
    fn from(e: SymError) -> Self {
        DecodeError::Sym(e.to_string())
    }
}

#[derive(Debug)]
pub struct AudioBuffer {
    /// Source-level normalization, measured once off the playback thread.
    pub loudness: crate::Loudness,
    /// Interleaved stereo f32, pre-touched.
    pub samples: Vec<f32>,
    pub frames: u64,
    pub sample_rate: u32,
}

impl AudioBuffer {
    pub fn stereo_at(&self, frame: f64) -> (f32, f32) {
        if self.frames == 0 {
            return (0.0, 0.0);
        }
        let max = (self.frames - 1) as f64;
        if frame < 0.0 || frame > max {
            return (0.0, 0.0);
        }
        let i0 = frame.floor() as usize;
        let t = (frame - i0 as f64) as f32;
        let last = self.frames as usize - 1;
        let interpolate = |channel| {
            let before = self.samples[i0.saturating_sub(1) * 2 + channel];
            let current = self.samples[i0 * 2 + channel];
            let after = self.samples[(i0 + 1).min(last) * 2 + channel];
            let following = self.samples[(i0 + 2).min(last) * 2 + channel];
            let slope = (after - before) * 0.5;
            let quadratic = before - current * 2.5 + after * 2.0 - following * 0.5;
            let cubic = (following - before) * 0.5 + (current - after) * 1.5;
            ((cubic * t + quadratic) * t + slope) * t + current
        };
        (interpolate(0), interpolate(1))
    }
}

pub fn decode_file(path: &Path) -> Result<Arc<AudioBuffer>, DecodeError> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe().format(
        &hint,
        mss,
        &FormatOptions::default(),
        &MetadataOptions::default(),
    )?;
    let mut format = probed.format;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(DecodeError::NoTrack)?;
    let track_id = track.id;
    let mut sr = None;
    let mut decoder =
        symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default())?;

    let capacity = track
        .codec_params
        .n_frames
        .unwrap_or(0)
        .min(48_000 * 60 * 30) as usize
        * 2;
    let mut samples = Vec::with_capacity(capacity);

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(error) => return Err(error.into()),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(buf) => {
                let rate = buf.spec().rate;
                let channels = buf.spec().channels.count();
                if rate == 0 || channels == 0 || sr.is_some_and(|previous| previous != rate) {
                    return Err(DecodeError::Sym("invalid or changing audio format".into()));
                }
                sr = Some(rate);
                // SampleBuffer handles every Symphonia sample type, including
                // 24-bit PCM. The old match silently discarded those formats.
                let mut pcm = SampleBuffer::<f32>::new(buf.capacity() as u64, *buf.spec());
                pcm.copy_interleaved_ref(buf);
                for frame in pcm.samples().chunks_exact(channels) {
                    let l = frame[0];
                    let r = frame[usize::from(channels > 1)];
                    if !l.is_finite() || !r.is_finite() {
                        return Err(DecodeError::Sym("non-finite audio samples".into()));
                    }
                    samples.push(l);
                    samples.push(r);
                }
            }
            Err(SymError::DecodeError(_)) => continue,
            Err(error) => return Err(error.into()),
        }
    }

    let frames = samples.len() / 2;
    if frames == 0 {
        return Err(DecodeError::Empty);
    }
    // Pre-touch so the audio thread does not page-fault.
    let mut acc = 0.0f32;
    for chunk in samples.chunks(1024) {
        acc += chunk.first().copied().unwrap_or(0.0);
    }
    std::hint::black_box(acc);

    Ok(Arc::new(AudioBuffer {
        loudness: crate::Loudness::measure(&samples, sr.ok_or(DecodeError::Empty)?),
        samples,
        frames: frames as u64,
        sample_rate: sr.ok_or(DecodeError::Empty)?,
    }))
}
