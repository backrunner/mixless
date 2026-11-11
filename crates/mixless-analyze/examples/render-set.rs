//! Continuous offline set with actual carried deck state and four-stem PCM.
//! render-set ANALYSES PATHS PLANS MODELS CACHE OUTPUT.wav
//! MIXLESS_AUDIT_LIBRARY may name a SQLite backup to reuse verified legacy stem identities.
use mixless_engine::{Engine, EngineConfig};
use mixless_protocol::{Command, DeckId, MixPlan, TrackAnalysis};
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 6 {
        return Err("Expected ANALYSES PATHS PLANS MODELS CACHE OUTPUT.wav".into());
    }
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let paths: Vec<String> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    let plans: Vec<MixPlan> = serde_json::from_slice(&std::fs::read(&args[2])?)?;
    if tracks.len() < 2 || paths.len() != tracks.len() || plans.len() + 1 != tracks.len() {
        return Err("Mismatched set inputs".into());
    }
    let processor = mixless_stems::Processor::new((&args[3]).into(), (&args[4]).into(), false);
    let library = std::env::var_os("MIXLESS_AUDIT_LIBRARY")
        .map(|path| mixless_library::Library::open(Path::new(&path)))
        .transpose()?;
    let engine = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48000,
        block_frames: 256,
    })?;
    let load = |index: usize, deck| -> Result<(), Box<dyn Error>> {
        let track = &tracks[index];
        engine.load_file(
            deck,
            track.track_id,
            Path::new(&paths[index]),
            String::new(),
            String::new(),
        )?;
        engine.set_bpm(deck, track.tempo.global_bpm);
        let source = engine
            .stem_source(deck, track.track_id)
            .ok_or("Missing source")?;
        let hash = format!(
            "blake3-{}",
            blake3::hash(&std::fs::read(&paths[index])?).to_hex()
        );
        // Match desktop's metadata-independent cache identity. Older audits
        // may still have a full-file cache, so retain that read-only fallback.
        let audio_hash = if let Some(library) = &library {
            library.stem_hash(Path::new(&paths[index]))?
        } else {
            mixless_library::stem_content_hash(Path::new(&paths[index]))?
        };
        let pcm = match processor.playback(&audio_hash, source.frames, source.sample_rate)? {
            Some(pcm) => pcm,
            None => processor
                .playback(&hash, source.frames, source.sample_rate)?
                .ok_or("Missing four-stem cache")?,
        };
        if !pcm.has_bass() || !engine.attach_stems(deck, track.track_id, &source, pcm)? {
            return Err("Four-stem attachment failed".into());
        }
        // Match desktop's reset_deck. Engine loads intentionally preserve
        // mixer controls; the outgoing fader from the previous use is closed.
        for cmd in [
            Command::SetRate { deck, rate: 1. },
            Command::SetPitchSemitones {
                deck,
                semitones: 0.,
            },
            Command::SetChannelGain { deck, db: 0. },
            Command::SetBalance { deck, value: 0. },
            Command::SetChannelFader { deck, value: 1. },
            Command::SetChannelFilter { deck, amount: 0. },
            Command::SetFxSend { deck, value: 0. },
            Command::SetKeyLock { deck, on: true },
            Command::SetReverse { deck, on: false },
            Command::SetLoopBeats {
                deck,
                beats: 4.,
                on: false,
            },
        ] {
            engine.dispatch(cmd)?;
        }
        for band in [
            mixless_protocol::EqBand::Low,
            mixless_protocol::EqBand::Mid,
            mixless_protocol::EqBand::High,
        ] {
            engine.dispatch(Command::SetEq { deck, band, db: 0. })?;
            engine.dispatch(Command::SetEqKill {
                deck,
                band,
                on: false,
            })?;
        }
        Ok(())
    };
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[5])?;
    let mut wav = hound::WavWriter::new(
        std::io::BufWriter::new(file),
        hound::WavSpec {
            channels: 2,
            sample_rate: 48000,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    let mut frames = 0u64;
    let mut peak = 0f32;
    let mut write = |audio: Vec<f32>| -> Result<(), Box<dyn Error>> {
        frames += audio.len() as u64 / 2;
        for sample in audio {
            if !sample.is_finite() || sample.abs() > 1. {
                return Err("Invalid rendered sample".into());
            }
            peak = peak.max(sample.abs());
            wav.write_sample(sample)?;
        }
        Ok(())
    };
    load(0, DeckId::A)?;
    engine.dispatch(Command::SetCrossfader { value: -1. })?;
    engine.dispatch(Command::SetKeyLock {
        deck: DeckId::A,
        on: true,
    })?;
    engine.dispatch(Command::PlayPause { deck: DeckId::A })?;
    let mut outgoing = DeckId::A;
    for (i, plan) in plans.into_iter().enumerate() {
        let incoming = if outgoing == DeckId::A {
            DeckId::B
        } else {
            DeckId::A
        };
        if plan
            .summary
            .as_ref()
            .is_none_or(|s| s.pair != (tracks[i].track_id, tracks[i + 1].track_id))
        {
            return Err("Plan pair mismatch".into());
        }
        let state = engine.snapshot();
        let deck = state.deck(outgoing);
        if (deck.rate - plan.outgoing_offset.rate).abs() > 0.002
            || (deck.sounding_pitch_semitones() - plan.outgoing_offset.pitch_semitones).abs() > 0.02
        {
            return Err("Carried performance offset differs from plan".into());
        }
        let source_sec = deck.frame as f32 / deck.src_sample_rate as f32;
        let limit =
            ((plan.t_in_a - source_sec).max(0.) / deck.rate + plan.duration_sec() + 10.) * 48000.;
        let expected = plan.incoming_offset_end;
        let expected_end = plan.t_end_b;
        load(i + 1, incoming)?;
        engine.load_plan_on(plan, outgoing)?;
        let mut elapsed = 0;
        let mut audible_frames = 0;
        while engine.snapshot().automix_on {
            let audio = engine.render_offline(256);
            audible_frames += audio
                .chunks_exact(2)
                .filter(|frame| frame.iter().any(|s| s.abs() > 0.0001))
                .count();
            write(audio)?;
            elapsed += 256;
            if elapsed as f32 > limit {
                return Err("Set handoff did not complete".into());
            }
        }
        if audible_frames < 4800 {
            return Err(format!(
                "Pair {i} advanced its clock without audible output ({audible_frames} frames)"
            )
            .into());
        }
        let snapshot = engine.snapshot();
        let deck = snapshot.deck(incoming);
        if !deck.playing {
            return Err("Incoming deck did not continue".into());
        }
        if deck.fader <= 0.001 || deck.stem_gain.iter().all(|g| *g <= 0.001) {
            return Err("Incoming deck completed with its signal closed".into());
        }
        if (deck.sounding_pitch_semitones() - expected.pitch_semitones).abs() > 0.02
            || (deck.rate - expected.rate).abs() > 0.002
            || (deck.frame as f32 / deck.src_sample_rate as f32 - expected_end).abs() > 0.03
        {
            return Err("Completed handoff differs from its final source/pitch/rate state".into());
        }
        println!(
            "{} -> {} completed: source={:.3} pitch={:.2} rate={:.6}, audible={:.2}s",
            tracks[i].track_id.0,
            tracks[i + 1].track_id.0,
            deck.frame as f32 / deck.src_sample_rate as f32,
            deck.sounding_pitch_semitones(),
            deck.rate,
            audible_frames as f32 / 48000.,
        );
        outgoing = incoming;
    }
    let snap = engine.snapshot();
    let deck = snap.deck(outgoing);
    let remaining = ((deck.frames.saturating_sub(deck.frame)) as f64
        / deck.src_sample_rate as f64
        / deck.rate as f64
        * 48000.)
        .ceil() as usize;
    for start in (0..remaining).step_by(256) {
        write(engine.render_offline((remaining - start).min(256)))?;
    }
    drop(write);
    wav.finalize()?;
    println!(
        "{:.2}s continuous set, peak={peak:.5}, all handoffs complete",
        frames as f64 / 48000.
    );
    Ok(())
}
