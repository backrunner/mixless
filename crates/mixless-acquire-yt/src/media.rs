//! In-process audio extraction. Only the pinned, statically linked audio codecs
//! are available; network access and external programs are disabled at build time.
#[cfg(test)]
pub(crate) mod tests;
mod transcode;

use crate::err;
use ffmpeg_next as av;
use mixless_acquire::AcquireError;
use std::{path::Path, sync::OnceLock, time::Instant};

fn initialize() -> Result<(), av::Error> {
    static INIT: OnceLock<Result<(), av::Error>> = OnceLock::new();
    *INIT.get_or_init(|| {
        av::init()?;
        av::log::set_level(av::log::Level::Error);
        Ok(())
    })
}

fn check_deadline(deadline: Instant) -> Result<(), av::Error> {
    if Instant::now() >= deadline {
        Err(av::Error::Exit)
    } else {
        Ok(())
    }
}

fn open(path: &Path, deadline: Instant) -> Result<av::format::context::Input, av::Error> {
    check_deadline(deadline)?;
    let mut options = av::Dictionary::new();
    options.set("protocol_whitelist", "file");
    options.set("max_streams", "16");
    options.set("probesize", "1048576");
    options.set("analyzeduration", "5000000");
    av::format::input_with_interrupt_and_dictionary(
        path,
        move || Instant::now() >= deadline,
        options,
    )
}

/// Keep AAC packets intact, or transcode alternate audio (e.g. WebM/Opus) to
/// AAC. The output stays private in the caller's staging directory until the
/// importer has a complete M4A file with recording metadata.
pub(super) fn prepare(source: &Path, output: &Path, deadline: Instant) -> Result<(), AcquireError> {
    let result = (|| {
        initialize()?;
        let mut input = open(source, deadline)?;
        let stream = input
            .streams()
            .best(av::media::Type::Audio)
            .ok_or(av::Error::StreamNotFound)?;
        let index = stream.index();
        let aac = stream.parameters().id() == av::codec::Id::AAC;
        let mut dest = av::format::output_as(output, "ipod")?;
        if aac {
            remux(&mut input, &mut dest, index, deadline)?;
        } else {
            transcode::run(&mut input, &mut dest, index, deadline)?;
        }
        Ok::<_, av::Error>(())
    })();
    result.map_err(|error| {
        // No failed/partial conversion can be mistaken for a reusable download.
        let _ = std::fs::remove_file(output);
        if error == av::Error::Exit {
            err("Audio processing timed out")
        } else {
            err(format!("Audio processing failed: {error}"))
        }
    })
}

fn remux(
    input: &mut av::format::context::Input,
    output: &mut av::format::context::Output,
    index: usize,
    deadline: Instant,
) -> Result<(), av::Error> {
    let stream = input.stream(index).ok_or(av::Error::StreamNotFound)?;
    let in_time_base = stream.time_base();
    let mut origin =
        (stream.start_time() != av::ffi::AV_NOPTS_VALUE).then_some(stream.start_time());
    let mut dest = output.add_stream(av::encoder::find(av::codec::Id::None))?;
    dest.set_parameters(stream.parameters());
    dest.set_time_base(in_time_base);
    // SAFETY: these parameters belong to the live destination stream. Let the
    // M4A muxer choose its own codec tag instead of retaining a transport tag.
    unsafe {
        (*dest.parameters().as_mut_ptr()).codec_tag = 0;
    }
    output.write_header()?;
    let out_time_base = output
        .stream(0)
        .ok_or(av::Error::StreamNotFound)?
        .time_base();
    let mut count = 0;
    loop {
        check_deadline(deadline)?;
        let mut packet = av::Packet::empty();
        match packet.read(input) {
            Ok(()) => {}
            Err(av::Error::Eof) => break,
            Err(error) => return Err(error),
        }
        if packet.stream() != index {
            continue;
        }
        let origin = *origin.get_or_insert(packet.dts().or(packet.pts()).unwrap_or(0));
        packet.set_pts(packet.pts().map(|ts| ts.saturating_sub(origin)));
        packet.set_dts(packet.dts().map(|ts| ts.saturating_sub(origin)));
        packet.rescale_ts(in_time_base, out_time_base);
        packet.set_stream(0);
        packet.set_position(-1);
        packet.write_interleaved(output)?;
        count += 1;
    }
    if count == 0 {
        return Err(av::Error::InvalidData);
    }
    output.write_trailer()
}

fn drained(error: av::Error) -> bool {
    error == av::Error::Eof
        || error
            == av::Error::Other {
                errno: av::error::EAGAIN,
            }
}
