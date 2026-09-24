//! Continuous offline set with actual carried deck state and four-stem PCM.
//! render-set ANALYSES PATHS PLANS MODELS CACHE OUTPUT.wav
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
        let pcm = processor
            .playback(&hash, source.frames, source.sample_rate)?
            .ok_or("Missing four-stem cache")?;
        if !pcm.has_bass() || !engine.attach_stems(deck, track.track_id, &source, pcm)? {
            return Err("Four-stem attachment failed".into());
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
            || (deck.pitch_semitones - plan.outgoing_offset.pitch_semitones).abs() > 0.02
        {
            return Err("Carried performance offset differs from plan".into());
        }
        let source_sec = deck.frame as f32 / deck.src_sample_rate as f32;
        let limit =
            ((plan.t_in_a - source_sec).max(0.) / deck.rate + plan.duration_sec() + 10.) * 48000.;
        load(i + 1, incoming)?;
        engine.load_plan_on(plan, outgoing)?;
        let mut elapsed = 0;
        while engine.snapshot().automix_on {
            write(engine.render_offline(256))?;
            elapsed += 256;
            if elapsed as f32 > limit {
                return Err("Set handoff did not complete".into());
            }
        }
        if !engine.snapshot().deck(incoming).playing {
            return Err("Incoming deck did not continue".into());
        }
        println!(
            "{} -> {} completed with carried state",
            tracks[i].track_id.0,
            tracks[i + 1].track_id.0
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
