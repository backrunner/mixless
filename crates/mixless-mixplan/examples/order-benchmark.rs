//! Time Best order on an immutable analysis export (no audio or library writes).
//! cargo run --release -p mixless-mixplan --example order-benchmark -- analyses.json [--stems]
use mixless_mixplan::{order::best_cycle_with_progress, PlannerOptions};
use mixless_protocol::TrackAnalysis;
use std::{error::Error, sync::Mutex, time::Instant};

#[path = "support/gpu.rs"]
mod gpu;

fn main() -> Result<(), Box<dyn Error>> {
    gpu::run(run)?
}

fn run() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let path = args.first().ok_or("expected analyses.json [--stems]")?;
    let tracks: Vec<TrackAnalysis> = serde_json::from_slice(&std::fs::read(path)?)?;
    let refs: Vec<_> = tracks.iter().collect();
    let n = tracks.len();
    let pairs = if n > 200 {
        0
    } else {
        n * if n <= 40 { n.saturating_sub(1) } else { 12 }
    };
    let starts = if n > 200 { 1 } else { n.min(16) };
    let times = Mutex::new([None; 3]);
    let start = Instant::now();
    let order = best_cycle_with_progress(
        &refs,
        &PlannerOptions {
            harmonic_key_shift: true,
            stem_playback: args.iter().any(|a| a == "--stems"),
            ..Default::default()
        },
        |done, _| {
            let elapsed = start.elapsed().as_secs_f64();
            let mut times = times.lock().unwrap();
            for (i, limit) in [n, n + pairs, n + pairs + starts].into_iter().enumerate() {
                if done >= limit && times[i].is_none() {
                    times[i] = Some(elapsed);
                    eprintln!(
                        "{}",
                        serde_json::json!({
                            "phase": (["rough", "pairs", "search"][i]),
                            "elapsed_seconds": elapsed,
                        })
                    );
                }
            }
        },
    );
    let elapsed = start.elapsed().as_secs_f64();
    let [rough, pairs, search] = times.into_inner().unwrap().map(|t| t.unwrap_or(elapsed));
    println!(
        "{}",
        serde_json::json!({
            "tracks": n, "seconds": elapsed, "rough_seconds": rough,
            "pairs_seconds": pairs - rough, "search_seconds": search - pairs,
            "chain_seconds": elapsed - search, "order": order,
        })
    );
    Ok(())
}
