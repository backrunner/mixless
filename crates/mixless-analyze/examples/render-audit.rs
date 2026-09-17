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
    let mut tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(&args[0])?)?;
    let paths: Vec<String> = serde_json::from_slice(&std::fs::read(&args[1])?)?;
    if tracks.len() < 2 || tracks.len() != paths.len() {
        return Err("Mismatched tracks/paths".into());
    }
    std::fs::create_dir_all(&args[2])?;
    let processor = (args.len() == 5)
        .then(|| mixless_stems::Processor::new((&args[3]).into(), (&args[4]).into(), false));
    let mut stem_hashes = vec![None; tracks.len()];
    if let Some(processor) = &processor {
        for (index, (track, path)) in tracks.iter_mut().zip(&paths).enumerate() {
            let raw = blake3::hash(&std::fs::read(path)?).to_hex().to_string();
            // Desktop library hashes include their algorithm prefix; older
            // standalone audits used the bare digest in separate caches.
            for hash in [format!("blake3-{raw}"), raw] {
                if let Some(stems) = processor.cached(&hash, track.duration_sec)? {
                    mixless_analyze::Analyzer::apply_stems(track, stems);
                    stem_hashes[index] = Some(hash);
                    break;
                }
            }
        }
    }
    let mut planner = Planner::with_options(PlannerOptions {
        harmonic_key_shift: true,
        strategy: std::env::var("MIXLESS_AUDIT_STRATEGY")
            .ok()
            .map(|s| serde_json::from_value(serde_json::Value::String(s)))
            .transpose()?,
        ..Default::default()
    });
    for i in 0..tracks.len() {
        let j = (i + 1) % tracks.len();
        planner.options.stem_playback = stem_hashes[i].is_some() && stem_hashes[j].is_some();
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
            if let Some((processor, hash)) = processor.as_ref().zip(stem_hashes[index].as_ref()) {
                let source = engine
                    .stem_source(deck, tracks[index].track_id)
                    .ok_or("Missing source")?;
                let audio = processor
                    .playback(hash, source.frames, source.sample_rate)?
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
        let preparing = std::time::Instant::now();
        engine.load_plan(plan)?;
        let preparation_ms = preparing.elapsed().as_secs_f64() * 1000.;
        if std::env::var("MIXLESS_AUDIT_GAIN").as_deref() == Ok("off") {
            engine.dispatch(Command::SetDeckLimiterGain {
                deck: DeckId::A,
                db: 0.,
            })?;
        }
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
        let mut gain_max = 0.0_f32;
        let mut levels = Vec::new();
        let mut window_power = 0.0_f64;
        let mut window_samples = 0;
        let mut window_gain = 0.0_f32;
        let mut total_samples = 0;
        for start in (0..frames).step_by(256) {
            let gain = engine
                .snapshot()
                .decks
                .iter()
                .map(|d| d.automix_gain_db)
                .fold(0., f32::max);
            gain_max = gain_max.max(gain);
            window_gain = window_gain.max(gain);
            for sample in engine.render_offline((frames - start).min(256)) {
                if !sample.is_finite() || sample.abs() > 1. {
                    return Err("Invalid rendered sample".into());
                }
                peak = peak.max(sample.abs());
                writer.write_sample(sample)?;
                window_power += (sample as f64).powi(2);
                window_samples += 1;
                total_samples += 1;
                if window_samples == 38_400 {
                    levels.push((
                        total_samples as f64 / 96_000.,
                        10. * (window_power / window_samples as f64).max(1e-12).log10(),
                        window_gain,
                    ));
                    window_samples = 0;
                    window_power = 0.;
                    window_gain = 0.;
                }
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
        std::fs::write(
            Path::new(&args[2]).join(format!("{}-{}.levels.json", a.track_id.0, b.track_id.0)),
            serde_json::to_vec(&levels)?,
        )?;
        let snapshot = engine.snapshot();
        if snapshot.automix_on || snapshot.automix_progress < 1. || !snapshot.decks[1].playing {
            return Err(format!("Pair {}→{} did not complete", a.track_id.0, b.track_id.0).into());
        }
        println!(
            "{}→{} {:?} {:.2}s peak={:.4} completed stem_blocks={} gain_max={:.2}dB prepare={:.1}ms",
            a.track_id.0, b.track_id.0, summary.strategy, duration, peak, stem_blocks, gain_max, preparation_ms
        );
    }
    Ok(())
}
