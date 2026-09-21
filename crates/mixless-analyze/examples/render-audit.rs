//! Render adjacent audited pairs without re-running spectral analysis or opening devices.
//! render-audit ANALYSES.json AUDIO_PATHS.json OUTPUT_DIRECTORY
use mixless_engine::{Engine, EngineConfig};
use mixless_mixplan::{short_handoff, PlanContext, Planner, PlannerOptions};
use mixless_protocol::{Command, Cue, CueKind, DeckId, TrackAnalysis};
use std::{collections::HashMap, error::Error, path::Path};

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
    let cues_map: HashMap<i64, Vec<Cue>> = match std::env::var("MIXLESS_AUDIT_CUES") {
        Ok(p) => serde_json::from_slice(&std::fs::read(p)?)?,
        Err(_) => HashMap::new(),
    };
    let processor = (args.len() == 5)
        .then(|| mixless_stems::Processor::new((&args[3]).into(), (&args[4]).into(), false));
    // Plan against the stored payloads exactly as the library audit does;
    // `apply_stems` rewrites bars/sections, so keep unmutated clones for
    // planning while the engine still receives the cached stem PCM.
    let planning_tracks = tracks.clone();
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
        let (a, b) = (&planning_tracks[i], &planning_tracks[j]);
        let filtered = |id, drop: CueKind| {
            cues_map
                .get(&id)
                .map(|c| {
                    c.iter()
                        .cloned()
                        .filter(|c| c.kind != drop)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let cues_out = filtered(a.track_id.0, CueKind::In);
        let cues_in = filtered(b.track_id.0, CueKind::Out);
        let ctx = PlanContext {
            outgoing: a,
            incoming: b,
            cues_out: &cues_out,
            cues_in: &cues_in,
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
        // Record the outgoing deck's source frame per rendered block so a
        // spinback's backwards pull can be verified from the actual render.
        let spin_window = plan.lanes.scratch_a.as_ref().map(|op| {
            (
                plan.clock.sample(op.on_bar),
                plan.clock.sample(op.off_bar),
                op.peak_delta_frames,
            )
        });
        let mut outgoing_frames: Vec<(f32, u64)> = Vec::new();
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
            let audio = engine.render_offline((frames - start).min(256));
            outgoing_frames.push((
                (start + audio.len() / 2) as f32 / 48_000.,
                engine.snapshot().decks[0].frame,
            ));
            for sample in audio {
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
        if let Some((on_sec, off_sec, delta)) = spin_window {
            let lo = before + on_sec;
            let hi = before + off_sec;
            let leg: Vec<u64> = outgoing_frames
                .iter()
                .filter(|(t, _)| *t >= lo && *t <= hi)
                .map(|(_, f)| *f)
                .collect();
            let backwards = leg.windows(2).filter(|w| w[1] < w[0]).count();
            let forwards = leg.windows(2).filter(|w| w[1] > w[0]).count();
            println!(
                "    spin window {on_sec:.2}–{off_sec:.2}s of plan (+{before:.2}s offset): {} samples, first={} last={} delta={delta} backwards_steps={backwards} forwards_steps={forwards}",
                leg.len(),
                leg.first().copied().unwrap_or(0),
                leg.last().copied().unwrap_or(0),
            );
        }
        println!(
            "{}→{} {:?} {:.2}s peak={:.4} completed stem_blocks={} gain_max={:.2}dB prepare={:.1}ms",
            a.track_id.0, b.track_id.0, summary.strategy, duration, peak, stem_blocks, gain_max, preparation_ms
        );
    }
    Ok(())
}
