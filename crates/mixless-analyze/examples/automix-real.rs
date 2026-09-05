//! Audition the same measured analysis and smooth policy used by desktop AUTO.
//! cargo run -p mixless-analyze --example automix -- a.wav b.wav preview.wav
use mixless_analyze::Analyzer;
use mixless_engine::{Engine, EngineConfig};
use mixless_mixplan::Planner;
use mixless_protocol::{Command, DeckId, TrackId};
use std::{error::Error, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("Usage: automix OUTGOING INCOMING OUTPUT.wav".into());
    }
    let engine = Engine::new(EngineConfig {
        offline: true,
        ..Default::default()
    })?;
    let analyzer = Analyzer::new();
    let mut analyses = Vec::new();
    for (i, deck) in [DeckId::A, DeckId::B].into_iter().enumerate() {
        let path = Path::new(&args[i]);
        let id = TrackId(i as i64 + 1);
        let analysis = analyzer.analyze_track(id, path)?;
        println!(
            "{deck:?}: {:.2} BPM, {}, {} measured bars",
            analysis.tempo.global_bpm,
            analysis.camelot.as_deref().unwrap_or("unknown key"),
            analysis.bars.len()
        );
        engine.load_file(deck, id, path, path.display().to_string(), String::new())?;
        engine.set_bpm(deck, analysis.tempo.global_bpm);
        analyses.push(analysis);
    }
    let plan = Planner::new().plan_pair(
        &analyses[0],
        &analyses[1],
        &[],
        &[],
        Default::default(),
        Default::default(),
    );
    if let Some(reason) = &plan.failure_reason {
        return Err(reason.clone().into());
    }
    let summary = plan.summary.as_ref().ok_or("No transition available")?;
    let n = summary.length_bars as f32;
    println!(
        "{:?} / {:?}, {} bars; A {:.3}–{:.3} s, B starts at {:.3} s",
        plan.transition_mode,
        summary.strategy,
        summary.length_bars,
        plan.t_in_a,
        plan.t_out_a,
        plan.t_in_b
    );
    println!(
        "Tempo {:.2} → {:.2} BPM; incoming native {:.2} BPM; handoff {:.3} s into transition",
        plan.master_bpm.sample(0.),
        plan.master_bpm.sample(n),
        analyses[1].tempo.global_bpm,
        plan.clock.sample(plan.handoff_bar.unwrap_or(n))
    );
    // Include surrounding music so the pace and energy change can be judged.
    let before = plan.t_in_a.min(4.);
    let after =
        ((analyses[1].duration_sec - plan.t_end_b) / plan.incoming_offset_end.rate).clamp(0., 8.);
    let frames =
        ((before + plan.duration_sec() + after) * engine.sample_rate() as f32).ceil() as usize;
    engine.dispatch(Command::Jog {
        deck: DeckId::A,
        delta_frames: (plan.t_in_a - before) * analyses[0].sample_rate as f32,
    })?;
    engine.render_offline(1);
    engine.dispatch(Command::SetCrossfader { value: -1. })?;
    engine.dispatch(Command::PlayPause { deck: DeckId::A })?;
    engine.load_plan(plan)?;
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
    let mut peak = 0f32;
    for start in (0..frames).step_by(256) {
        for sample in engine.render_offline((frames - start).min(256)) {
            if !sample.is_finite() {
                return Err("Non-finite audio output".into());
            }
            peak = peak.max(sample.abs());
            writer.write_sample(sample)?;
        }
    }
    writer.finalize()?;
    println!(
        "Wrote {} ({:.2} s, peak {:.2} dBFS)",
        args[2].to_string_lossy(),
        frames as f32 / engine.sample_rate() as f32,
        20. * peak.max(1e-12).log10()
    );
    Ok(())
}
