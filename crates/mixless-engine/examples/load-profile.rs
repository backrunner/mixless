//! Measure the decode/normalization and first-waveform path without opening audio devices.
//! cargo run --locked -p mixless-engine --example load-profile -- TRACK...
use std::{error::Error, path::Path, time::Instant};

fn main() -> Result<(), Box<dyn Error>> {
    let paths: Vec<_> = std::env::args_os().skip(1).collect();
    if paths.is_empty() {
        return Err("Expected one or more audio files".into());
    }
    for (i, path) in paths.iter().enumerate() {
        let start = Instant::now();
        let audio = mixless_engine::decode_file(Path::new(path))?;
        let decoded = start.elapsed();
        let waveform_start = Instant::now();
        let wave = mixless_engine::compute_preview_waveform(&audio, 4096);
        println!(
            "file={} duration={:.2}s decode_and_loudness={:.1}ms preview={:.1}ms bins={} lufs={:?} trim={:.2}dB peak_after_trim={:?}dBTP",
            i + 1,
            audio.frames as f64 / audio.sample_rate as f64,
            decoded.as_secs_f64() * 1000.,
            waveform_start.elapsed().as_secs_f64() * 1000.,
            wave.columns,
            audio.loudness.integrated_lufs,
            audio.loudness.gain_db,
            audio.loudness.true_peak_db.map(|peak| peak + audio.loudness.gain_db)
        );
    }
    Ok(())
}
