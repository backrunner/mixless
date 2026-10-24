use super::*;
use std::{fs, time::Duration};

pub(crate) fn write_wav(path: &Path, rate: u32, channels: u16, seconds: f64) {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate: rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for i in 0..(rate as f64 * seconds) as usize {
        for channel in 0..channels {
            let phase =
                i as f64 * std::f64::consts::TAU * (440. * (channel + 1) as f64) / rate as f64;
            writer.write_sample((10000. * phase.sin()) as i16).unwrap();
        }
    }
    writer.finalize().unwrap();
}

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(10)
}

fn packets(path: &Path) -> Vec<Vec<u8>> {
    let mut input = open(path, deadline()).unwrap();
    input
        .packets()
        .map(|(_, packet)| packet.data().unwrap().to_vec())
        .collect()
}

fn check_audio(path: &Path, rate: u32, seconds: f64, mono: bool) {
    let audio = mixless_engine::decode_file(path).unwrap();
    assert_eq!(audio.sample_rate, rate);
    assert!((audio.frames as f64 / rate as f64 - seconds).abs() < 0.06);
    assert!(audio.samples.iter().all(|sample| sample.is_finite()));
    let strength = |channel: usize, hz: f64| {
        let mut real = 0.;
        let mut imag = 0.;
        for (i, frame) in audio.samples.chunks_exact(2).enumerate() {
            let phase = i as f64 * std::f64::consts::TAU * hz / rate as f64;
            real += frame[channel] as f64 * phase.cos();
            imag += frame[channel] as f64 * phase.sin();
        }
        real.hypot(imag) / audio.frames as f64
    };
    assert!(strength(0, 440.) > 0.10, "left signal lost");
    assert!(
        strength(1, if mono { 440. } else { 880. }) > 0.10,
        "right signal lost"
    );
    if !mono {
        assert!(strength(0, 880.) < 0.01, "channels mixed");
        assert!(strength(1, 440.) < 0.01, "channels mixed");
    }
}

#[test]
fn native_aac_encoding_and_remux_preserve_audio_and_metadata() {
    let dir = tempfile::tempdir().unwrap();
    for (rate, channels) in [(44100, 1), (48000, 2)] {
        let source = dir.path().join("input.wav");
        let encoded = dir.path().join("encoded.m4a");
        let remuxed = dir.path().join("remuxed.m4a");
        write_wav(&source, rate, channels, 1.25);
        prepare(&source, &encoded, deadline()).unwrap();
        prepare(&encoded, &remuxed, deadline()).unwrap();
        assert_eq!(
            packets(&encoded),
            packets(&remuxed),
            "AAC must not be re-encoded"
        );
        check_audio(&remuxed, rate, 1.25, channels == 1);
        let job = crate::ResolveJob {
            spotify_id: "a".repeat(22),
            title: "音乐 Test".into(),
            artist: "Artist".into(),
            duration_ms: 1250,
            isrc: Some("GBAAA2600001".into()),
        };
        crate::metadata::write(&remuxed, &job).unwrap();
        use lofty::{file::TaggedFileExt, prelude::Accessor, tag::ItemKey};
        let tagged = lofty::probe::Probe::open(&remuxed).unwrap().read().unwrap();
        let tag = tagged.primary_tag().unwrap();
        assert_eq!(tag.title().as_deref(), Some(job.title.as_str()));
        assert_eq!(tag.artist().as_deref(), Some(job.artist.as_str()));
        assert_eq!(tag.get_string(&ItemKey::Isrc), job.isrc.as_deref());
        assert_eq!(packets(&encoded), packets(&remuxed));
    }
}

#[test]
fn webm_opus_and_hls_transport_audio_convert_without_external_tools() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["tone-opus.webm", "tone-aac.ts", "tone-aac.aac"] {
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(name);
        let output = dir.path().join("converted.m4a");
        prepare(&source, &output, deadline()).unwrap_or_else(|error| panic!("{name}: {error}"));
        check_audio(&output, 48000, 1., false);
    }
}

#[test]
fn invalid_inputs_and_expired_deadlines_leave_no_partial_output() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("bad.webm");
    let output = dir.path().join("audio.m4a");
    for bytes in [b"".as_slice(), b"not audio"] {
        fs::write(&source, bytes).unwrap();
        assert!(prepare(&source, &output, deadline()).is_err());
        assert!(!output.exists());
    }
    write_wav(&source, 44100, 2, 1.);
    assert_eq!(
        prepare(&source, &output, Instant::now())
            .unwrap_err()
            .to_string(),
        "Audio processing timed out"
    );
    assert!(!output.exists());
}

#[test]
fn linked_library_has_only_the_expected_portable_audio_configuration() {
    initialize().unwrap();
    // SAFETY: FFmpeg returns immutable, static NUL-terminated build strings.
    let config = unsafe { std::ffi::CStr::from_ptr(av::ffi::avcodec_configuration()) }
        .to_str()
        .unwrap();
    let license = unsafe { std::ffi::CStr::from_ptr(av::ffi::avcodec_license()) }
        .to_str()
        .unwrap();
    for flag in [
        "--disable-network",
        "--disable-autodetect",
        "--disable-programs",
        "--disable-gpl",
        "--disable-nonfree",
    ] {
        assert!(config.contains(flag), "missing {flag}: {config}");
    }
    assert!(!config.contains("-march=native"));
    assert!(!config.contains("--enable-libx265"));
    #[cfg(target_os = "macos")]
    assert!(config.contains("-mmacosx-version-min=12.0"));
    assert!(license.contains("LGPL"));
    assert!(av::encoder::find(av::codec::Id::AAC).is_some());
    assert!(av::decoder::find(av::codec::Id::OPUS).is_some());
    assert!(av::decoder::find(av::codec::Id::H264).is_none());
}
