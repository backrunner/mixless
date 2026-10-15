//! Strict, bounded cache decoding; damaged packets must never shift stem timing.
use super::codec_error;
use crate::{
    Result, StorageFormat, check,
    storage::{MAX_FRAMES, RATE},
};
use std::{fs::File, path::Path};
use symphonia::core::{
    audio::SampleBuffer,
    codecs::{
        CODEC_TYPE_FLAC, CODEC_TYPE_MP3, CODEC_TYPE_PCM_F32LE, CodecParameters, DecoderOptions,
    },
    errors::Error as SymError,
    formats::{FormatOptions, FormatReader},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};

fn open(path: &Path, kind: StorageFormat) -> Result<(Box<dyn FormatReader>, u32, CodecParameters)> {
    let mut hint = Hint::new();
    hint.with_extension(kind.extension());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            MediaSourceStream::new(Box::new(File::open(path)?), Default::default()),
            &FormatOptions {
                enable_gapless: true,
                ..Default::default()
            },
            &MetadataOptions::default(),
        )
        .map_err(|e| codec_error(format!("probe: {e}")))?;
    let track = probed
        .format
        .default_track()
        .ok_or_else(|| codec_error("Missing stem audio"))?;
    let params = track.codec_params.clone();
    let id = track.id;
    let codec = match kind {
        StorageFormat::Wav => CODEC_TYPE_PCM_F32LE,
        StorageFormat::Flac => CODEC_TYPE_FLAC,
        StorageFormat::Mp3 => CODEC_TYPE_MP3,
    };
    if params.codec != codec
        || params.sample_rate != Some(RATE)
        || params.channels.is_none_or(|c| c.count() != 2)
        || params.n_frames.is_none_or(|n| n == 0 || n > MAX_FRAMES)
        || (kind == StorageFormat::Flac && params.bits_per_sample != Some(24))
        || (kind == StorageFormat::Mp3 && (params.delay.is_none() || params.padding.is_none()))
    {
        return Err(codec_error("Unsupported or unaligned stem format"));
    }
    Ok((probed.format, id, params))
}

pub(crate) fn probe(path: &Path, kind: StorageFormat) -> Result<u64> {
    Ok(open(path, kind)?.2.n_frames.unwrap())
}

pub(crate) fn read(
    path: &Path,
    kind: StorageFormat,
    frames: u64,
    gain: f32,
    active: &impl Fn() -> bool,
) -> Result<Vec<f32>> {
    let (mut format, track, params) = open(path, kind)?;
    if params.n_frames != Some(frames) || !gain.is_finite() || !(1.0..=128.).contains(&gain) {
        return Err(codec_error("Stem duration or gain mismatch"));
    }
    let mut decoder = symphonia::default::get_codecs()
        .make(&params, &DecoderOptions { verify: true })
        .map_err(codec_error)?;
    let mut samples = Vec::with_capacity(frames as usize * 2);
    loop {
        check(active)?;
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(codec_error(e)),
        };
        if packet.track_id() != track {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| codec_error(format!("decode at {} frames: {e}", samples.len() / 2)))?;
        if decoded.spec().rate != RATE || decoded.spec().channels.count() != 2 {
            return Err(codec_error("Stem format changed during decoding"));
        }
        let mut pcm = SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec());
        pcm.copy_interleaved_ref(decoded);
        if samples.len() + pcm.len() > frames as usize * 2 {
            return Err(codec_error("Stem has extra audio frames"));
        }
        for &sample in pcm.samples() {
            let sample = sample * gain;
            if !sample.is_finite() {
                return Err(codec_error("Non-finite stem audio"));
            }
            samples.push(sample);
        }
    }
    if samples.len() != frames as usize * 2 {
        return Err(codec_error(format!(
            "Incomplete stem audio: expected {frames} frames, decoded {}",
            samples.len() / 2
        )));
    }
    if decoder.finalize().verify_ok == Some(false) {
        return Err(codec_error("Stem audio checksum mismatch"));
    }
    Ok(samples)
}
