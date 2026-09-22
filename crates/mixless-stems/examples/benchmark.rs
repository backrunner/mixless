//! Same native models and preprocessing as the desktop, bypassing audio caches.
#[cfg(stems_ort)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{path::PathBuf, time::Instant};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err("Usage: benchmark AUDIO MODEL_DIR OUTPUT_DIR [START_SECONDS DURATION_SECONDS RUNS]\nSet MIXLESS_STEMS_BACKEND=auto|mlx|coreml|cpu, MIXLESS_ORT_THREADS, and optional MIXLESS_ORT_PROFILE_DIR.".into());
    }
    let start: f64 = args.get(3).map(|s| s.parse()).transpose()?.unwrap_or(0.);
    let duration: f64 = args.get(4).map(|s| s.parse()).transpose()?.unwrap_or(30.);
    let runs: usize = args.get(5).map(|s| s.parse()).transpose()?.unwrap_or(3);
    if !start.is_finite() || !duration.is_finite() || start < 0. || duration <= 0. || runs == 0 {
        return Err("Invalid benchmark interval or run count".into());
    }
    let audio = mixless_engine::decode_file(std::path::Path::new(&args[0]))?;
    let first = (start * audio.sample_rate as f64) as usize * 2;
    let end =
        (((start + duration) * audio.sample_rate as f64) as usize * 2).min(audio.samples.len());
    let samples = audio
        .samples
        .get(first..end)
        .filter(|s| !s.is_empty())
        .ok_or("Empty interval")?;
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    let at = Instant::now();
    // A benchmark always uses already downloaded, checksum-verified models.
    let mut inference = mixless_stems::Inference::load(
        std::path::Path::new(&args[1]),
        None,
        false,
        &mut |_| {},
        &|| true,
    )?;
    let load = at.elapsed().as_secs_f64();
    let reference = std::env::var_os("MIXLESS_BENCH_REFERENCE_DIR")
        .map(
            |root| -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
                ["vocals", "drums", "instruments"]
                    .iter()
                    .map(|name| {
                        let reader = hound::WavReader::open(
                            PathBuf::from(&root).join(format!("{name}.wav")),
                        )?;
                        Ok(reader
                            .into_samples::<f32>()
                            .collect::<Result<Vec<_>, _>>()?)
                    })
                    .collect()
            },
        )
        .transpose()?;
    let mut results = Vec::new();
    for run in 0..runs {
        let at = Instant::now();
        let stems = inference.separate(samples, audio.sample_rate, &mut |_| {}, &|| true)?;
        let separation = at.elapsed().as_secs_f64();
        let at = Instant::now();
        let vocals = inference.transcribe(&stems.audio[0], "vocals", &mut |_| {}, &|| true)?;
        let instruments =
            inference.transcribe(&stems.audio[2], "instruments", &mut |_| {}, &|| true)?;
        let notes = at.elapsed().as_secs_f64();
        let mut max_difference = 0f32;
        if let Some(reference) = &reference {
            for (expected, actual) in reference.iter().zip(&stems.audio) {
                if expected.len() != actual.len() {
                    return Err("Reference length differs".into());
                }
                for (&a, &b) in expected.iter().zip(actual) {
                    max_difference = max_difference.max((a - b).abs());
                }
            }
            if max_difference >= 1e-3 {
                return Err(format!(
                    "Backend/CPU difference {max_difference} exceeds 1e-3 on run {run}"
                )
                .into());
            }
        }
        let result = serde_json::json!({"run":run,"separation_sec":separation,"notes_sec":notes,"total_sec":separation+notes,"notes":vocals.len()+instruments.len(),"residual_rms":stems.residual_rms,"reference_max_abs":reference.as_ref().map(|_| max_difference)});
        eprintln!("{result}");
        results.push(result);
        if run + 1 == runs {
            for (name, samples) in ["vocals", "drums", "instruments"].iter().zip(&stems.audio) {
                let mut wav = hound::WavWriter::create(
                    output.join(format!("{name}.wav")),
                    hound::WavSpec {
                        channels: 2,
                        sample_rate: 44100,
                        bits_per_sample: 32,
                        sample_format: hound::SampleFormat::Float,
                    },
                )?;
                for &sample in samples {
                    wav.write_sample(sample)?;
                }
                wav.finalize()?;
            }
            std::fs::write(
                output.join("notes.json"),
                serde_json::to_vec(
                    &serde_json::json!({"vocals":vocals,"instruments":instruments}),
                )?,
            )?;
        }
    }
    let profiles = inference.finish_profiling()?;
    let result = serde_json::json!({"backends":inference.backends(),"load_sec":load,"audio_sec":samples.len() as f64/2./audio.sample_rate as f64,"runs":results,"profiles":profiles});
    std::fs::write(
        output.join("benchmark.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    println!("{result}");
    Ok(())
}
#[cfg(not(stems_ort))]
fn main() {
    eprintln!("Native model inference is unavailable on this target");
    std::process::exit(1);
}
