use super::codec_error;
use crate::{Result, StorageFormat, check, storage::RATE};
use std::{
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
};

pub(crate) fn write(
    path: &Path,
    samples: &[f32],
    format: StorageFormat,
    active: &impl Fn() -> bool,
) -> Result<f32> {
    check(active)?;
    if samples.is_empty()
        || samples.len() % 2 != 0
        || samples.len() as u64 / 2 > crate::storage::MAX_FRAMES
        || samples.iter().any(|s| !s.is_finite() || s.abs() > 64.)
    {
        return Err(codec_error("Invalid stem PCM"));
    }
    let peak = samples.iter().fold(0f32, |p, s| p.max(s.abs()));
    // Power-of-two scaling is exactly reversible in f32. Keep integer and
    // lossy encoders below full scale without clipping separator overshoots.
    let gain = if format == StorageFormat::Wav || peak < 1. {
        1.
    } else {
        2f32.powf(peak.log2().floor() + 1.)
    };
    match format {
        StorageFormat::Wav => {
            let mut wav = hound::WavWriter::create(
                path,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: RATE,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )?;
            for chunk in samples.chunks(16384) {
                check(active)?;
                for &s in chunk {
                    wav.write_sample(s)?;
                }
            }
            wav.finalize()?;
        }
        StorageFormat::Flac => super::flac::encode(path, samples, gain, active)?,
        StorageFormat::Mp3 => mp3(path, samples, gain, active)?,
    }
    Ok(gain)
}

fn mp3(path: &Path, samples: &[f32], gain: f32, active: &impl Fn() -> bool) -> Result<()> {
    use mp3lame_encoder::{Bitrate, Builder, FlushGap, InterleavedPcm, Quality};
    let mut encoder = Builder::new()
        .ok_or_else(|| codec_error("Cannot create MP3 encoder"))?
        .with_num_channels(2)
        .map_err(codec_error)?
        .with_sample_rate(RATE)
        .map_err(codec_error)?
        .with_output_sample_rate(std::num::NonZeroU32::new(RATE))
        .map_err(codec_error)?
        .with_brate(Bitrate::Kbps320)
        .map_err(codec_error)?
        .with_quality(Quality::Best)
        .map_err(codec_error)?
        .with_to_write_vbr_tag(true)
        .map_err(codec_error)?
        .build()
        .map_err(codec_error)?;
    let mut file = File::create(path)?;
    let mut output = Vec::with_capacity(mp3lame_encoder::max_required_buffer_size(8192));
    for chunk in samples.chunks(16384) {
        check(active)?;
        let pcm: Vec<f32> = chunk.iter().map(|s| s / gain).collect();
        output.clear();
        encoder
            .encode_to_vec(InterleavedPcm(&pcm), &mut output)
            .map_err(codec_error)?;
        file.write_all(&output)?;
    }
    output.clear();
    encoder
        .flush_to_vec::<FlushGap>(&mut output)
        .map_err(codec_error)?;
    file.write_all(&output)?;
    // Replace LAME's reserved Info frame with final sample count/delay/padding.
    // Symphonia's gapless decoder uses these values for sample-exact alignment.
    let mut tag = Vec::with_capacity(encoder.lame_tag_size());
    encoder
        .lame_tag_encode_to_vec(&mut tag)
        .ok_or_else(|| codec_error("Missing MP3 gapless tag"))?;
    file.seek(SeekFrom::Start(encoder.id3v2_tag_size() as u64))?;
    file.write_all(&tag)?;
    Ok(())
}
