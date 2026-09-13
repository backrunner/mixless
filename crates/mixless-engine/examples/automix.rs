//! Render a transition preview without opening an audio device.
//! cargo run -p mixless-engine --example automix -- a.wav b.wav mix.wav 128 126
use mixless_engine::{Engine, EngineConfig};
use mixless_protocol::{Command, DeckId, PerformanceOffset, TempoMap, TrackAnalysis, TrackId};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 5 {
        return Err("Usage: automix OUTGOING INCOMING OUTPUT.wav BPM_A BPM_B".into());
    }
    let engine = Engine::new(EngineConfig {
        offline: true,
        ..Default::default()
    })?;
    let mut analyses = Vec::new();
    for (i, deck) in [DeckId::A, DeckId::B].into_iter().enumerate() {
        let bpm: f32 = args[i + 3].to_string_lossy().parse()?;
        if !bpm.is_finite() || !(20.0..=400.0).contains(&bpm) {
            return Err("BPM must be between 20 and 400".into());
        }
        engine.load_file(
            deck,
            TrackId(i as i64 + 1),
            std::path::Path::new(&args[i]),
            format!("Deck {deck:?}"),
            String::new(),
        )?;
        engine.set_bpm(deck, bpm);
        let snapshot = engine.snapshot();
        let d = snapshot.deck(deck);
        analyses.push(TrackAnalysis {
            track_id: d.track_id.unwrap(),
            duration_sec: d.frames as f32 / d.src_sample_rate as f32,
            sample_rate: d.src_sample_rate,
            tempo: TempoMap {
                global_bpm: bpm,
                meter_num: 4,
                meter_den: 4,
                ..Default::default()
            },
            key: None,
            camelot: None,
            key_confidence: 0.,
            sections: vec![],
            bars: vec![],
            phrase_boundaries: vec![],
            mix_regions: vec![],
            moments: vec![],
            stems: None,
            waveform_path: None,
            partial: true,
        });
    }
    let plan = mixless_mixplan::Planner::new().plan_pair(
        &analyses[0],
        &analyses[1],
        &[],
        &[],
        PerformanceOffset::identity(),
        PerformanceOffset::identity(),
    );
    if let Some(reason) = &plan.failure_reason {
        return Err(reason.clone().into());
    }
    let summary = plan.summary.as_ref().unwrap();
    println!(
        "{:?}, {} bars, {:.2} s overlap",
        summary.strategy,
        summary.length_bars,
        plan.clock.sample(summary.length_bars as f32)
    );
    let frames = ((plan.duration_sec() + 2.0) * engine.sample_rate() as f32).ceil() as usize;
    // Seek exactly in source time while paused; then arm the plan at that position.
    engine.dispatch(Command::Jog {
        deck: DeckId::A,
        delta_frames: plan.t_in_a * analyses[0].sample_rate as f32,
    })?;
    engine.render_offline(1);
    engine.dispatch(Command::SetCrossfader { value: -1. })?;
    engine.dispatch(Command::PlayPause { deck: DeckId::A })?;
    engine.load_plan(plan)?;
    // Don't silently replace an existing recording or either input file.
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&args[2])?;
    let mut writer = hound::WavWriter::new(
        std::io::BufWriter::new(output),
        hound::WavSpec {
            channels: 2,
            sample_rate: engine.sample_rate(),
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        },
    )?;
    for start in (0..frames).step_by(256) {
        for sample in engine.render_offline((frames - start).min(256)) {
            writer.write_sample(sample)?;
        }
    }
    writer.finalize()?;
    println!("Wrote {}", args[2].to_string_lossy());
    Ok(())
}
