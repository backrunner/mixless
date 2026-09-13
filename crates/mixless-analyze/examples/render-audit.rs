//! Render adjacent audited pairs without re-running spectral analysis or opening devices.
//! render-audit ANALYSES.json AUDIO_PATHS.json OUTPUT_DIRECTORY
use mixless_engine::{Engine, EngineConfig};
use mixless_mixplan::{short_handoff, PlanContext, Planner, PlannerOptions};
use mixless_protocol::{Command, DeckId, TrackAnalysis};
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 && args.len() != 5 {
        return Err(
            "Expected ANALYSES.json AUDIO_PATHS.json OUTPUT_DIRECTORY [MODEL_DIR STEM_CACHE]"
                .into(),
        );
    }
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let paths: Vec<String> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    if tracks.len() < 2 || tracks.len() != paths.len() {
        return Err("Mismatched tracks/paths".into());
    }
    std::fs::create_dir_all(&args[2])?;
    let processor = (args.len() == 5)
        .then(|| mixless_stems::Processor::new((&args[3]).into(), (&args[4]).into(), false));
    let planner = Planner::with_options(PlannerOptions {
        harmonic_key_shift: true,
        strategy: std::env::var("MIXLESS_AUDIT_STRATEGY")
            .ok()
            .map(|s| serde_json::from_value(serde_json::Value::String(s)))
            .transpose()?,
        ..Default::default()
    });
    for i in 0..tracks.len() {
        let j = (i + 1) % tracks.len();
        let (a, b) = (&tracks[i], &tracks[j]);
        let ctx = PlanContext {
            outgoing: a,
            incoming: b,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        };
        let mut plan = planner.plan_next(&ctx);
        if plan.failure_reason.is_some() {
            plan = short_handoff(&ctx, 0.);
        }
        if let Some(reason) = &plan.failure_reason {
            return Err(reason.clone().into());
        }
        let engine = Engine::new(EngineConfig {
            offline: true,
            sample_rate: 48_000,
            block_frames: 256,
        })?;
        for (deck, index) in [(DeckId::A, i), (DeckId::B, j)] {
            engine.load_file(
                deck,
                tracks[index].track_id,
                Path::new(&paths[index]),
                String::new(),
                String::new(),
            )?;
            engine.set_bpm(deck, tracks[index].tempo.global_bpm);
            if let Some(processor) = &processor {
                let hash = blake3::hash(&std::fs::read(&paths[index])?)
                    .to_hex()
                    .to_string();
                let source = engine
                    .stem_source(deck, tracks[index].track_id)
                    .ok_or("Missing source")?;
                let audio = processor
                    .playback(&hash, source.frames, source.sample_rate)?
                    .ok_or("Missing complete stem PCM cache")?;
                if !engine.attach_stems(deck, tracks[index].track_id, &source, audio)? {
                    return Err("Stem publication rejected".into());
                }
            }
        }
        let before = plan.t_in_a.min(4.);
        let duration = before + plan.duration_sec() + 4.;
        engine.cue_loaded_track(
            DeckId::A,
            a.track_id,
            ((plan.t_in_a - before) * a.sample_rate as f32).round() as u64,
        )?;
        engine.dispatch(Command::SetCrossfader { value: -1. })?;
        engine.dispatch(Command::PlayPause { deck: DeckId::A })?;
        let summary = plan.summary.clone().ok_or("Missing summary")?;
        std::fs::write(
            Path::new(&args[2]).join(format!("{}-{}.json", a.track_id.0, b.track_id.0)),
            serde_json::to_vec_pretty(&plan)?,
        )?;
        engine.load_plan(plan)?;
        let output = Path::new(&args[2]).join(format!("{}-{}.wav", a.track_id.0, b.track_id.0));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        let mut writer = hound::WavWriter::new(
            std::io::BufWriter::new(file),
            hound::WavSpec {
                channels: 2,
                sample_rate: 48_000,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        let frames = (duration * 48_000.).ceil() as usize;
        let mut peak = 0f32;
        let mut stem_blocks = 0;
        for start in (0..frames).step_by(256) {
            for sample in engine.render_offline((frames - start).min(256)) {
                if !sample.is_finite() || sample.abs() > 1. {
                    return Err("Invalid rendered sample".into());
                }
                peak = peak.max(sample.abs());
                writer.write_sample(sample)?;
            }
            if engine
                .snapshot()
                .decks
                .iter()
                .any(|d| d.stems_ready && d.stem_gain.iter().any(|g| *g < 0.99))
            {
                stem_blocks += 1;
            }
        }
        writer.finalize()?;
        let snapshot = engine.snapshot();
        if snapshot.automix_on || snapshot.automix_progress < 1. || !snapshot.decks[1].playing {
            return Err(format!("Pair {}→{} did not complete", a.track_id.0, b.track_id.0).into());
        }
        println!(
            "{}→{} {:?} {:.2}s peak={:.4} completed stem_blocks={}",
            a.track_id.0, b.track_id.0, summary.strategy, duration, peak, stem_blocks
        );
    }
    Ok(())
}
