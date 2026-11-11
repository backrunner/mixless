//! Strict, bounded cache decoding; damaged packets must never shift stem timing.
use super::codec_error;
use crate::{
    check,
    storage::{MAX_FRAMES, RATE},
    Result, StorageFormat,
};
use std::{fs::File, path::Path};
use symphonia::core::{
    codecs::audio::{
        well_known::{CODEC_ID_FLAC, CODEC_ID_MP3, CODEC_ID_PCM_F32LE},
        AudioCodecParameters, AudioDecoderOptions,
    },
    errors::Error as SymError,
    formats::{probe::Hint, FormatOptions, FormatReader, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

fn open(
    path: &Path,
    kind: StorageFormat,
) -> Result<(Box<dyn FormatReader>, u32, AudioCodecParameters, u64)> {
    let mut hint = Hint::new();
    hint.with_extension(kind.extension());
    let probed = symphonia::default::get_probe()
        .probe(
            &hint,
            MediaSourceStream::new(Box::new(File::open(path)?), Default::default()),
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| codec_error(format!("probe: {e}")))?;
    let track = probed
        .default_track(TrackType::Audio)
        .ok_or_else(|| codec_error("Missing stem audio"))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| codec_error("Missing stem audio codec"))?
        .clone();
    let id = track.id;
    let codec = match kind {
        StorageFormat::Wav => CODEC_ID_PCM_F32LE,
        StorageFormat::Flac => CODEC_ID_FLAC,
        StorageFormat::Mp3 => CODEC_ID_MP3,
    };
    if params.codec != codec
        || params.sample_rate != Some(RATE)
        || params.channels.as_ref().is_none_or(|c| c.count() != 2)
        || track.num_frames.is_none_or(|n| n == 0 || n > MAX_FRAMES)
        || (kind == StorageFormat::Flac && params.bits_per_sample != Some(24))
        || (kind == StorageFormat::Mp3 && (track.delay.is_none() || track.padding.is_none()))
    {
        return Err(codec_error("Unsupported or unaligned stem format"));
    }
    let frames = track.num_frames.unwrap();
    Ok((probed, id, params, frames))
}

pub(crate) fn probe(path: &Path, kind: StorageFormat) -> Result<u64> {
    Ok(open(path, kind)?.3)
}

pub(crate) fn read(
    path: &Path,
    kind: StorageFormat,
    frames: u64,
    gain: f32,
    active: &impl Fn() -> bool,
) -> Result<Vec<f32>> {
    let (mut format, track, params, expected_frames) = open(path, kind)?;
    if expected_frames != frames || !gain.is_finite() || !(1.0..=128.).contains(&gain) {
        return Err(codec_error("Stem duration or gain mismatch"));
    }
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(
            &params,
            &AudioDecoderOptions::default().verify(true).gapless(true),
        )
        .map_err(codec_error)?;
    let mut samples = Vec::with_capacity(frames as usize * 2);
    loop {
        check(active)?;
        let packet = match format.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(codec_error(e)),
        };
        if packet.track_id != track {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| codec_error(format!("decode at {} frames: {e}", samples.len() / 2)))?;
        if decoded.spec().rate() != RATE || decoded.spec().channels().count() != 2 {
            return Err(codec_error("Stem format changed during decoding"));
        }
        let mut pcm = vec![0f32; decoded.samples_interleaved()];
        decoded.copy_to_slice_interleaved(&mut pcm);
        if samples.len() + pcm.len() > frames as usize * 2 {
            return Err(codec_error("Stem has extra audio frames"));
        }
        for &sample in &pcm {
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
