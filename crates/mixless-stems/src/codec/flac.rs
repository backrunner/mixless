//! libFLAC level 8, streamed off the audio thread with bounded conversion memory.
use super::codec_error;
use crate::{Result, check, storage::RATE};
use libflac_sys::*;
use std::{
    ffi::{CStr, c_void},
    fs::File,
    io::{Seek, SeekFrom, Write},
    path::Path,
    ptr::NonNull,
};

struct Encoder(NonNull<FLAC__StreamEncoder>);
impl Drop for Encoder {
    fn drop(&mut self) {
        // SAFETY: this guard uniquely owns the encoder from new until delete.
        unsafe { FLAC__stream_encoder_delete(self.0.as_ptr()) };
    }
}

pub(super) fn encode(
    path: &Path,
    samples: &[f32],
    gain: f32,
    active: &impl Fn() -> bool,
) -> Result<()> {
    // The file stays at a stable address until the synchronous encoder is dropped.
    let mut file = File::create(path)?;
    // SAFETY: allocation returns either null or a uniquely owned encoder.
    let encoder = Encoder(
        NonNull::new(unsafe { FLAC__stream_encoder_new() })
            .ok_or_else(|| codec_error("Cannot create FLAC encoder"))?,
    );
    let ptr = encoder.0.as_ptr();
    // SAFETY: all calls use this live encoder; callbacks only borrow `file`
    // synchronously and no callback or pointer escapes this function.
    unsafe {
        let configured = FLAC__stream_encoder_set_channels(ptr, 2)
            & FLAC__stream_encoder_set_bits_per_sample(ptr, 24)
            & FLAC__stream_encoder_set_sample_rate(ptr, RATE)
            & FLAC__stream_encoder_set_compression_level(ptr, 8)
            & FLAC__stream_encoder_set_total_samples_estimate(ptr, samples.len() as u64 / 2)
            & FLAC__stream_encoder_set_verify(ptr, 1);
        if configured == 0 {
            return Err(codec_error("Cannot configure FLAC encoder"));
        }
        let status = FLAC__stream_encoder_init_stream(
            ptr,
            Some(write),
            Some(seek),
            Some(tell),
            None,
            (&mut file as *mut File).cast(),
        );
        if status != FLAC__STREAM_ENCODER_INIT_STATUS_OK {
            return Err(codec_error(format!(
                "Cannot initialize FLAC encoder ({status})"
            )));
        }
    }
    let mut pcm = Vec::with_capacity(16384);
    for block in samples.chunks(16384) {
        check(active)?;
        pcm.clear();
        pcm.extend(block.iter().map(|&s| {
            ((s as f64 / gain as f64 * 8388608.).round() as i32).clamp(-8388608, 8388607)
        }));
        // SAFETY: pcm contains exactly two valid i32 samples per frame. It stays
        // alive and unchanged throughout this synchronous call.
        if unsafe {
            FLAC__stream_encoder_process_interleaved(ptr, pcm.as_ptr(), pcm.len() as u32 / 2)
        } == 0
        {
            return Err(state_error(ptr));
        }
    }
    check(active)?;
    // SAFETY: the encoder and its callback file are still alive.
    if unsafe { FLAC__stream_encoder_finish(ptr) } == 0 {
        return Err(state_error(ptr));
    }
    Ok(())
}

fn state_error(ptr: *mut FLAC__StreamEncoder) -> crate::Error {
    // SAFETY: callers hold a live encoder; libFLAC returns a static C string.
    let text = unsafe { CStr::from_ptr(FLAC__stream_encoder_get_resolved_state_string(ptr)) };
    codec_error(format!("FLAC: {}", text.to_string_lossy()))
}

unsafe extern "C" fn write(
    _: *const FLAC__StreamEncoder,
    buffer: *const u8,
    bytes: usize,
    _: u32,
    _: u32,
    client: *mut c_void,
) -> FLAC__StreamEncoderWriteStatus {
    // SAFETY: init_stream supplies the live File; libFLAC supplies `bytes`
    // readable bytes for the duration of this callback.
    let file = unsafe { &mut *client.cast::<File>() };
    if bytes == 0 {
        return FLAC__STREAM_ENCODER_WRITE_STATUS_OK;
    }
    let data = unsafe { std::slice::from_raw_parts(buffer, bytes) };
    if file.write_all(data).is_ok() {
        FLAC__STREAM_ENCODER_WRITE_STATUS_OK
    } else {
        FLAC__STREAM_ENCODER_WRITE_STATUS_FATAL_ERROR
    }
}
unsafe extern "C" fn seek(
    _: *const FLAC__StreamEncoder,
    offset: u64,
    client: *mut c_void,
) -> FLAC__StreamEncoderSeekStatus {
    // SAFETY: init_stream supplies the live File for this synchronous callback.
    let file = unsafe { &mut *client.cast::<File>() };
    if file.seek(SeekFrom::Start(offset)).is_ok() {
        FLAC__STREAM_ENCODER_SEEK_STATUS_OK
    } else {
        FLAC__STREAM_ENCODER_SEEK_STATUS_ERROR
    }
}
unsafe extern "C" fn tell(
    _: *const FLAC__StreamEncoder,
    offset: *mut u64,
    client: *mut c_void,
) -> FLAC__StreamEncoderTellStatus {
    // SAFETY: init_stream supplies the live File; libFLAC supplies a writable offset.
    let file = unsafe { &mut *client.cast::<File>() };
    match file.stream_position() {
        Ok(pos) => {
            unsafe { *offset = pos };
            FLAC__STREAM_ENCODER_TELL_STATUS_OK
        }
        Err(_) => FLAC__STREAM_ENCODER_TELL_STATUS_ERROR,
    }
}
