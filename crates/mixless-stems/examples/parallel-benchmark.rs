//! Exercise production session pooling and cache publication without a user library.
//! parallel-benchmark MODEL_DIR OUTPUT_DIR [JOBS SECONDS]
#[path = "parallel-benchmark/playback.rs"]
mod playback;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Instant,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("expected MODEL_DIR OUTPUT_DIR [JOBS SECONDS]".into());
    }
    let jobs: usize = args.get(2).map(|v| v.parse()).transpose()?.unwrap_or(4);
    let seconds: f32 = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(16.);
    if !(1..=16).contains(&jobs) || !seconds.is_finite() || !(1. ..=60.).contains(&seconds) {
        return Err("invalid workload".into());
    }
    let output = PathBuf::from(&args[1]);
    std::fs::create_dir_all(&output)?;
    let processor =
        mixless_stems::Processor::new(output.join("downloads"), output.join("cache"), false)
            .with_bundled_models(Some(PathBuf::from(&args[0])));
    let rendering = std::env::var_os("MIXLESS_BENCH_PLAYBACK_CACHE")
        .map(|cache| playback::start(PathBuf::from(cache), seconds))
        .transpose()?;
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let start = Instant::now();
    let results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..jobs).map(|i| {
            let (processor, active, peak) = (&processor, &active, &peak);
            scope.spawn(move || -> Result<serde_json::Value, String> {
                let hash = format!("parallel-benchmark-{i}-{seconds}");
                let frames = (44100. * seconds) as usize;
                let mut loaded = false;
                let mut waited = false;
                let mut loading_at = None;
                let at = Instant::now();
                let result = processor.analyze(&hash, seconds, || {
                    loaded = true;
                    let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(count, Ordering::SeqCst);
                    let samples = (0..frames).flat_map(|f| {
                        let t = f as f32 / 44100.;
                        let signal = (t * std::f32::consts::TAU * (220. + i as f32 * 37.)).sin() * 0.1
                            + (t * std::f32::consts::TAU * 70.).sin() * 0.05;
                        [signal, signal * 0.8]
                    }).collect();
                    Ok(Arc::new(mixless_engine::AudioBuffer { samples, frames: frames as u64, sample_rate: 44100, loudness: Default::default() }))
                }, &mut |p| match p {
                    mixless_stems::Progress::WaitingForResources => waited = true,
                    mixless_stems::Progress::Loading => { loading_at.get_or_insert(at.elapsed().as_secs_f64()); },
                    _ => {},
                }, &|| true);
                if loaded { active.fetch_sub(1, Ordering::SeqCst); }
                let analysis = result.map_err(|e| e.to_string())?;
                if !mixless_stems::is_current(&analysis) { return Err("invalid evidence".into()); }
                let playback = processor.playback(&hash, frames as u64, 44100).map_err(|e| e.to_string())?;
                if playback.is_none() { return Err("missing playback cache".into()); }
                Ok(serde_json::json!({"job":i,"seconds":at.elapsed().as_secs_f64(),"notes":analysis.notes.len(),"cached":!loaded,"waiting_for_resources":waited,"loading_started_sec":loading_at,"cache":processor.cache_path(&hash)}))
            })
        }).collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("benchmark worker panicked"))
            .collect::<Result<Vec<_>, _>>()
    });
    let wall = start.elapsed().as_secs_f64();
    let rendered = rendering.map(playback::Run::finish).transpose()?;
    let result = serde_json::json!({"workers":processor.parallelism(),"jobs":jobs,"audio_seconds":seconds,"wall_seconds":wall,"peak_active":peak.load(Ordering::Relaxed),"results":results?,"render":rendered});
    std::fs::write(
        output.join("benchmark.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
