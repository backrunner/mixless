//! Native diagnostic entry point; the desktop calls the same Processor API.
use std::{path::PathBuf, sync::Arc, time::Instant};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        return Err(
            "Usage: mixless-stems AUDIO MODEL_DIR CACHE_DIR [START_SECONDS DURATION_SECONDS]"
                .into(),
        );
    }
    let path = PathBuf::from(&args[0]);
    let mut audio = mixless_engine::decode_file(&path)?;
    let start: f32 = args.get(3).map(|v| v.parse()).transpose()?.unwrap_or(0.);
    let duration: f32 = args
        .get(4)
        .map(|v| v.parse())
        .transpose()?
        .unwrap_or(audio.frames as f32 / audio.sample_rate as f32 - start);
    if start < 0. || duration <= 0. || !start.is_finite() || !duration.is_finite() {
        return Err("Invalid audio interval".into());
    }
    let first = (start * audio.sample_rate as f32) as usize;
    let end = ((start + duration) * audio.sample_rate as f32) as usize;
    let sr = audio.sample_rate;
    let samples = audio
        .samples
        .get(first * 2..end.min(audio.frames as usize) * 2)
        .ok_or("Interval outside audio")?
        .to_vec();
    audio = Arc::new(mixless_engine::AudioBuffer {
        frames: (samples.len() / 2) as u64,
        sample_rate: sr,
        samples,
        loudness: Default::default(),
    });
    let mut hash = blake3::Hasher::new();
    hash.update(&sr.to_le_bytes());
    for s in &audio.samples {
        hash.update(&s.to_le_bytes());
    }
    let hash = hash.finalize().to_hex().to_string();
    let processor = mixless_stems::Processor::new((&args[1]).into(), (&args[2]).into(), true);
    let started = Instant::now();
    let evidence = processor.analyze(
        &hash,
        audio.frames as f32 / sr as f32,
        || Ok(audio),
        &mut |p| eprintln!("{p:?}"),
        &|| true,
    )?;
    println!(
        "{}",
        serde_json::json!({"duration_sec":evidence.duration_sec,"elapsed_sec":started.elapsed().as_secs_f64(),"frames":evidence.frames.len(),"notes":evidence.notes.len(),"residual_rms":evidence.residual_rms,"cache":processor.cache_path(&hash)})
    );
    Ok(())
}
