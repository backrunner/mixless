use super::{av, check_deadline, drained};
use std::time::Instant;

struct Transcoder {
    decoder: av::codec::decoder::Audio,
    encoder: av::codec::encoder::Audio,
    filter: av::filter::Graph,
    samples: i64,
    output_time_base: av::Rational,
    deadline: Instant,
}

pub(super) fn run(
    input: &mut av::format::context::Input,
    output: &mut av::format::context::Output,
    index: usize,
    deadline: Instant,
) -> Result<(), av::Error> {
    let stream = input.stream(index).ok_or(av::Error::StreamNotFound)?;
    let context = av::codec::context::Context::from_parameters(stream.parameters())?;
    let mut decoder = context.decoder().audio()?;
    if decoder.rate() == 0 || decoder.channels() == 0 {
        return Err(av::Error::InvalidData);
    }
    if decoder.channel_layout().is_empty() {
        decoder.set_channel_layout(av::ChannelLayout::default(decoder.channels() as i32));
    }
    let codec = av::encoder::find(av::codec::Id::AAC)
        .ok_or(av::Error::EncoderNotFound)?
        .audio()?;
    let mut encoder = av::codec::context::Context::new_with_codec(*codec)
        .encoder()
        .audio()?;
    let layout = if decoder.channels() == 1 {
        av::ChannelLayout::MONO
    } else {
        av::ChannelLayout::STEREO
    };
    encoder.set_rate(decoder.rate() as i32);
    encoder.set_channel_layout(layout);
    encoder.set_format(av::format::Sample::F32(av::format::sample::Type::Planar));
    encoder.set_bit_rate(if layout.channels() == 1 {
        128_000
    } else {
        256_000
    });
    encoder.set_time_base((1, decoder.rate() as i32));
    encoder.set_flags(av::codec::flag::Flags::GLOBAL_HEADER);
    let encoder = encoder.open_as(codec)?;
    let mut dest = output.add_stream(codec)?;
    dest.set_time_base(encoder.time_base());
    dest.set_parameters(&encoder);

    let mut filter = av::filter::Graph::new();
    let args = format!(
        "time_base=1/{}:sample_rate={}:sample_fmt={}:channel_layout=0x{:x}",
        decoder.rate(),
        decoder.rate(),
        decoder.format().name(),
        decoder.channel_layout().bits()
    );
    filter.add(
        &av::filter::find("abuffer").ok_or(av::Error::FilterNotFound)?,
        "in",
        &args,
    )?;
    filter.add(
        &av::filter::find("abuffersink").ok_or(av::Error::FilterNotFound)?,
        "out",
        "",
    )?;
    // Express the contract through aformat. FFmpeg 9 changed the buffersink
    // option types, so older scalar sink setters can leave a mismatched frame
    // format that is unsafe to hand to an encoder expecting planar floats.
    let conversion = format!(
        "aformat=sample_fmts=fltp:sample_rates={}:channel_layouts={}",
        encoder.rate(),
        if layout.channels() == 1 {
            "mono"
        } else {
            "stereo"
        }
    );
    filter
        .output("in", 0)?
        .input("out", 0)?
        .parse(&conversion)?;
    filter.validate()?;
    filter
        .get("out")
        .ok_or(av::Error::FilterNotFound)?
        .sink()
        .set_frame_size(encoder.frame_size());
    output.write_header()?;
    let mut state = Transcoder {
        decoder,
        encoder,
        filter,
        samples: 0,
        deadline,
        output_time_base: output
            .stream(0)
            .ok_or(av::Error::StreamNotFound)?
            .time_base(),
    };
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
        state.decoder.send_packet(&packet)?;
        state.decoded(output)?;
    }
    state.decoder.send_eof()?;
    state.decoded(output)?;
    if state.samples == 0 {
        return Err(av::Error::InvalidData);
    }
    state
        .filter
        .get("in")
        .ok_or(av::Error::FilterNotFound)?
        .source()
        .flush()?;
    state.filtered(output)?;
    state.encoder.send_eof()?;
    state.encoded(output)?;
    output.write_trailer()
}

impl Transcoder {
    fn decoded(&mut self, output: &mut av::format::context::Output) -> Result<(), av::Error> {
        loop {
            check_deadline(self.deadline)?;
            let mut frame = av::frame::Audio::empty();
            match self.decoder.receive_frame(&mut frame) {
                Ok(()) => {}
                Err(error) if drained(error) => break,
                Err(error) => return Err(error),
            }
            frame.set_pts(Some(self.samples));
            self.samples += frame.samples() as i64;
            self.filter
                .get("in")
                .ok_or(av::Error::FilterNotFound)?
                .source()
                .add(&frame)?;
            self.filtered(output)?;
        }
        Ok(())
    }

    fn filtered(&mut self, output: &mut av::format::context::Output) -> Result<(), av::Error> {
        loop {
            check_deadline(self.deadline)?;
            let mut frame = av::frame::Audio::empty();
            match self
                .filter
                .get("out")
                .ok_or(av::Error::FilterNotFound)?
                .sink()
                .frame(&mut frame)
            {
                Ok(()) => {}
                Err(error) if drained(error) => break,
                Err(error) => return Err(error),
            }
            if frame.format() != self.encoder.format()
                || frame.rate() != self.encoder.rate()
                || frame.channel_layout() != self.encoder.channel_layout()
            {
                return Err(av::Error::InvalidData);
            }
            self.encoder.send_frame(&frame)?;
            self.encoded(output)?;
        }
        Ok(())
    }

    fn encoded(&mut self, output: &mut av::format::context::Output) -> Result<(), av::Error> {
        loop {
            check_deadline(self.deadline)?;
            let mut packet = av::Packet::empty();
            match self.encoder.receive_packet(&mut packet) {
                Ok(()) => {}
                Err(error) if drained(error) => break,
                Err(error) => return Err(error),
            }
            packet.rescale_ts(self.encoder.time_base(), self.output_time_base);
            packet.set_stream(0);
            packet.set_position(-1);
            packet.write_interleaved(output)?;
        }
        Ok(())
    }
}
