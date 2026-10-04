use super::*;

#[test]
fn source_normalization_reaches_both_master_and_cue_without_moving_manual_gain() {
    let render = |amplitude: f32| {
        let engine = test_engine(48_000);
        let mut buffer = sine_buffer(48_000, 1000., 3.);
        let audio = Arc::get_mut(&mut buffer).unwrap();
        for x in &mut audio.samples {
            *x *= amplitude;
        }
        audio.loudness = crate::Loudness::measure(&audio.samples, audio.sample_rate);
        let wave = Arc::new(crate::compute_preview_waveform(&buffer, 256));
        engine
            .load_buffer_if(
                DeckId::A,
                TrackId(1),
                buffer,
                wave,
                String::new(),
                String::new(),
                || true,
            )
            .unwrap();
        engine
            .dispatch(Command::SetCrossfader { value: -1. })
            .unwrap();
        engine
            .dispatch(Command::SetPfl {
                deck: DeckId::A,
                on: true,
            })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine.render_offline(48_000);
        let (producer, mut consumer) = rtrb::RingBuffer::new(4096);
        let mut rt = engine.rt.lock().unwrap();
        rt.cue_output = Some(device::CueWriter::new(producer, 48_000, 48_000));
        let mut output = vec![0.; 4096 * 2];
        engine.shared.process_block(&mut rt, &mut output, 2);
        let cue: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
            .flatten()
            .collect();
        assert_eq!(engine.snapshot().decks[0].gain_db, 0.);
        [energy(&output[4000..]), energy(&cue[4000..])]
    };
    let quiet = render(0.2);
    let loud = render(1.6);
    for (quiet, loud) in quiet.into_iter().zip(loud) {
        assert!(quiet > 0.0001);
        assert!((10. * (quiet / loud).log10()).abs() < 0.1);
    }
}

#[test]
fn headphone_bus_is_pre_fader_and_independent_of_master() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine
        .dispatch(Command::SetChannelFader {
            deck: DeckId::A,
            value: 0.0,
        })
        .unwrap();
    engine.dispatch(Command::SetMaster { value: 0.0 }).unwrap();
    engine
        .dispatch(Command::SetPfl {
            deck: DeckId::A,
            on: true,
        })
        .unwrap();
    let (producer, mut consumer) = rtrb::RingBuffer::new(8192);
    let mut rt = engine.rt.lock().unwrap();
    rt.cue_output = Some(device::CueWriter::new(producer, 48_000, 48_000));
    let mut output = vec![0.0; 4096 * 2];
    engine.shared.process_block(&mut rt, &mut output, 2);
    assert!(energy(&output[4000..]) < 1e-8);
    let headphones: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
        .flatten()
        .collect();
    assert!(energy(&headphones[4000..]) > 0.001);
    engine
        .dispatch(Command::SetPfl {
            deck: DeckId::A,
            on: false,
        })
        .unwrap();
    engine.shared.process_block(&mut rt, &mut output, 2);
    let headphones: Vec<_> = std::iter::from_fn(|| consumer.pop().ok())
        .flatten()
        .collect();
    assert!(energy(&headphones[4000..]) < 1e-8);
    assert!(!engine.snapshot().pfl_available);
}

#[test]
fn output_rate_converts_sources_without_changing_duration_or_pitch() {
    for (source_rate, output_rate) in [
        (44_100, 48_000),
        (48_000, 44_100),
        (96_000, 48_000),
        (44_100, 96_000),
        (48_000, 176_400),
        (48_000, 192_000),
    ] {
        let engine = test_engine(output_rate);
        let slot = &engine.shared.decks[0];
        let buffer = sine_buffer(source_rate, 1000., 4.);
        slot.src_sr.store(source_rate, Ordering::Relaxed);
        slot.frames.store(buffer.frames, Ordering::Relaxed);
        *slot.buffer.lock().unwrap() = Some(buffer);
        engine
            .dispatch(Command::SetCrossfader { value: -1. })
            .unwrap();
        engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        engine.render_offline(output_rate as usize / 10);
        let start = engine.snapshot().decks[0].frame;
        let output = left_channel(&engine, output_rate as usize);
        let end = engine.snapshot().decks[0].frame;
        assert!((end as i64 - start as i64 - source_rate as i64).abs() <= 2);
        assert!(output.iter().all(|sample| sample.is_finite()));
        let crossings = output
            .windows(2)
            .filter(|s| s[0] <= 0. && s[1] > 0.)
            .count();
        assert!(
            (crossings as i32 - 1000).abs() <= 2,
            "{source_rate} -> {output_rate}: {crossings}"
        );
        assert!(energy(&output) > 0.001);
    }
}

#[test]
#[ignore = "changes real CoreAudio physical output formats"]
#[cfg(target_os = "macos")]
fn macos_physical_output_format_smoke() {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    assert!(engine.audio_available());
    let snapshot = engine.snapshot();
    let status = snapshot.master_output.as_ref().unwrap();
    assert_eq!(status.sample_rate, snapshot.sample_rate);
    assert!(status.hardware_format.is_some());
    let device = device::output_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.name == snapshot.device_name)
        .unwrap();
    eprintln!(
        "CoreAudio {} current={} Hz {:?} supported={:?}",
        device.name, status.sample_rate, status.hardware_format, device.output_formats
    );
    let formats = device
        .output_formats
        .iter()
        .filter(|f| [44_100, 48_000, 96_000, 176_400, 192_000].contains(&f.sample_rate))
        .copied()
        .collect::<Vec<_>>();
    for format in formats {
        let config = device::AudioConfig {
            sample_rate: Some(format.sample_rate),
            bit_depth: Some(format.bit_depth),
            ..Default::default()
        };
        engine.configure_audio(config.clone()).unwrap();
        let snapshot = engine.snapshot();
        let status = snapshot.master_output.as_ref().unwrap();
        assert_eq!(snapshot.sample_rate, format.sample_rate);
        assert_eq!(status.sample_rate, format.sample_rate);
        assert!(status
            .hardware_format
            .as_ref()
            .unwrap()
            .starts_with(&format!("{}-bit", format.bit_depth)));
        assert_eq!(engine.audio_config(), config);
        eprintln!(
            "verified {} Hz {:?} client={}",
            status.sample_rate, status.hardware_format, status.stream_format
        );
        // Force failure AFTER applying a different physical rate; it must restore hardware.
        let other_rate = if format.sample_rate == 48_000 {
            44_100
        } else {
            48_000
        };
        if device
            .output_formats
            .iter()
            .any(|f| f.sample_rate == other_rate && f.bit_depth == format.bit_depth)
        {
            let mut invalid = config.clone();
            invalid.sample_rate = Some(other_rate);
            invalid.buffer_frames = Some(0);
            assert!(engine.configure_audio(invalid).is_err());
            assert_eq!(engine.audio_config(), config);
            assert!(engine.audio_available());
        }
    }
    let previous = engine.audio_config();
    let previous_status = engine.snapshot().master_output;
    assert!(engine
        .configure_audio(device::AudioConfig {
            bit_depth: Some(7),
            ..previous.clone()
        })
        .is_err());
    assert_eq!(engine.audio_config(), previous);
    assert_eq!(engine.snapshot().master_output, previous_status);
    // Restore the original physical format at the end of this hardware test.
    let bits = status
        .hardware_format
        .as_ref()
        .unwrap()
        .split('-')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    engine
        .configure_audio(device::AudioConfig {
            sample_rate: Some(status.sample_rate),
            bit_depth: Some(bits),
            ..Default::default()
        })
        .unwrap();
    engine
        .configure_audio(device::AudioConfig::default())
        .unwrap();
    assert_eq!(engine.snapshot().sample_rate, status.sample_rate);
}

#[test]
#[ignore = "opens real CoreAudio output device and changes its buffer size"]
#[cfg(target_os = "macos")]
fn macos_low_latency_buffer_smoke() {
    use cpal::traits::{DeviceTrait, HostTrait};
    let device = cpal::default_host().default_output_device().unwrap();
    let supported = device.default_output_config().unwrap();
    let (min, max) = match *supported.buffer_size() {
        cpal::SupportedBufferSize::Range { min, max } => (min, max),
        cpal::SupportedBufferSize::Unknown => panic!("device buffer range unavailable"),
    };
    // A sentinel prevents the initial snapshot from passing before a callback.
    let engine = Engine::new(EngineConfig {
        block_frames: 1,
        ..Default::default()
    })
    .unwrap();
    assert!(engine.audio_available());
    let wait_for_buffer = |expected| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while engine.snapshot().block_frames != expected && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let snapshot = engine.snapshot();
        assert!(engine.audio_available());
        assert_eq!(snapshot.block_frames, expected);
        eprintln!(
            "CoreAudio device={} buffer={} frames duration={:.2}ms",
            snapshot.device_name,
            snapshot.block_frames,
            snapshot.block_frames as f64 * 1000. / snapshot.sample_rate as f64
        );
    };
    wait_for_buffer(256_u32.clamp(min.max(1), max));
    for frames in [64, 128, 256, 512, 1024, 2048]
        .into_iter()
        .filter(|n| (min..=max).contains(n))
    {
        let config = device::AudioConfig {
            buffer_frames: Some(frames),
            ..Default::default()
        };
        engine.configure_audio(config.clone()).unwrap();
        wait_for_buffer(frames);
        assert_eq!(engine.audio_config(), config);
    }
    let previous = engine.audio_config();
    assert!(engine
        .configure_audio(device::AudioConfig {
            buffer_frames: Some(0),
            ..Default::default()
        })
        .is_err());
    assert_eq!(engine.audio_config(), previous);
    wait_for_buffer(
        previous
            .buffer_frames
            .unwrap_or(256_u32.clamp(min.max(1), max)),
    );
    engine
        .configure_audio(device::AudioConfig::default())
        .unwrap();
    wait_for_buffer(256_u32.clamp(min.max(1), max));
}

#[test]
#[ignore = "opens real CoreAudio output devices"]
fn audio_device_reconfiguration_smoke() {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    assert!(engine.audio_available());
    let master = engine.snapshot().device_name;
    let mut config = device::AudioConfig {
        buffer_frames: Some(256),
        ..Default::default()
    };
    engine.configure_audio(config.clone()).unwrap();
    let other = device::output_devices()
        .unwrap()
        .into_iter()
        .find(|d| d.name != master);
    if let Some(other) = other {
        config.cue_device = Some(other.name);
        engine.configure_audio(config.clone()).unwrap();
        assert!(engine.snapshot().pfl_available);
    } else {
        eprintln!("Only one output device available; separate-device PFL smoke skipped");
    }
    let mut invalid = config.clone();
    invalid.master_device = Some("mixless-missing-device-test".into());
    assert!(engine.configure_audio(invalid).is_err());
    assert_eq!(engine.audio_config(), config);
    assert!(engine.audio_available());
    invalid = config.clone();
    invalid.cue_device = Some(master);
    assert!(engine.configure_audio(invalid).is_err());
    assert_eq!(engine.audio_config(), config);
    engine
        .configure_audio(device::AudioConfig::default())
        .unwrap();
    assert!(!engine.snapshot().pfl_available);
}

#[test]
fn source_lock_contention_does_not_interrupt_playback() {
    let engine = test_engine(48_000);
    engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    engine.render_offline(2048);
    let start = engine.snapshot().decks[0].frame;
    let _guard = engine.shared.decks[0].buffer.lock().unwrap();
    let output = engine.render_offline(256);
    assert!(energy(&output) > 0.001);
    assert_eq!(engine.snapshot().decks[0].frame, start + 256);
    assert_eq!(engine.snapshot().xrun_count, 0);
}

#[test]
fn offline_silence_is_finite_zero() {
    let eng = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    let out = eng.render_offline(512);
    assert_eq!(out.len(), 1024);
    assert!(out.iter().all(|s| s.is_finite() && *s == 0.0));
}

#[test]
fn two_decks_mix_without_nan() {
    let eng = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    {
        let slot = &eng.shared.decks[0];
        *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 220.0, 1.0));
        slot.frames.store(48_000, Ordering::Relaxed);
        slot.src_sr.store(48_000, Ordering::Relaxed);
        slot.playing.store(true, Ordering::Relaxed);
    }
    {
        let slot = &eng.shared.decks[1];
        *slot.buffer.lock().unwrap() = Some(sine_buffer(48_000, 330.0, 1.0));
        slot.frames.store(48_000, Ordering::Relaxed);
        slot.src_sr.store(48_000, Ordering::Relaxed);
        slot.playing.store(true, Ordering::Relaxed);
    }
    eng.dispatch(Command::SetCrossfader { value: 0.0 }).unwrap();
    let out = eng.render_offline(1024);
    assert!(out.iter().all(|s| s.is_finite()));
    let energy: f32 = out.iter().map(|s| s * s).sum();
    assert!(energy > 0.01, "expected audible energy, got {energy}");
}

#[test]
fn dispatch_updates_snapshot_without_audio_thread() {
    let eng = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    eng.dispatch(Command::SetCrossfader { value: -1.0 })
        .unwrap();
    eng.dispatch(Command::SetMaster { value: 0.25 }).unwrap();
    let snap = eng.snapshot();
    assert!((snap.xfader - (-1.0)).abs() < 0.01);
    assert!((snap.master - 0.25).abs() < 0.01);
}
