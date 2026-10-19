//! Render real cached model output through the production offline engine.
use mixless_engine::{AudioBuffer, Engine, EngineConfig, StemBuffer};
use mixless_protocol::{Command, DeckId, StemKind, TrackId};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err(
            "Usage: review-playback AUDIO STEM_DIR OUTPUT_DIR START_SECONDS DURATION_SECONDS"
                .into(),
        );
    }
    let source = mixless_engine::decode_file(Path::new(&args[0]))?;
    if source.sample_rate != 44100 {
        return Err("Review expects 44.1 kHz source matching benchmark stems".into());
    }
    let first = (args[3].parse::<f64>()? * 44100.).round() as usize * 2;
    let length = (args[4].parse::<f64>()? * 44100.).round() as usize * 2;
    let audio = Arc::new(AudioBuffer {
        samples: source.samples[first..first + length].to_vec(),
        frames: (length / 2) as u64,
        sample_rate: 44100,
        loudness: Default::default(),
    });
    let read = |name: &str| -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let reader = hound::WavReader::open(Path::new(&args[1]).join(format!("{name}.wav")))?;
        Ok(reader.into_samples::<f32>().collect::<Result<_, _>>()?)
    };
    let stems = Arc::new(StemBuffer::with_bass(
        44100,
        read("vocals")?,
        read("drums")?,
        read("bass")?,
    )?);
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    let render = |attach: bool, gains: [f32; 4]| -> Result<Vec<f32>, Box<dyn std::error::Error>> {
        let engine = Engine::new(EngineConfig {
            offline: true,
            sample_rate: 44100,
            block_frames: 256,
        })?;
        engine.load_buffer_if(
            DeckId::A,
            TrackId(1),
            audio.clone(),
            Arc::new(mixless_engine::compute_preview_waveform(&audio, 32)),
            "review".into(),
            String::new(),
            || true,
        )?;
        if attach {
            engine.attach_stems(DeckId::A, TrackId(1), &audio, stems.clone())?;
        }
        for cmd in [
            Command::SetCrossfader { value: -1. },
            Command::SetMasterGain { db: 0. },
            Command::SetMaster { value: 1. },
            Command::SetChannelGain {
                deck: DeckId::A,
                db: -6.,
            },
            Command::SetChannelFader {
                deck: DeckId::A,
                value: 1.,
            },
        ] {
            engine.dispatch(cmd)?;
        }
        for (stem, value) in [
            StemKind::Vocals,
            StemKind::Drums,
            StemKind::Instruments,
            StemKind::Bass,
        ]
        .into_iter()
        .zip(gains)
        {
            engine.dispatch(Command::SetStemGain {
                deck: DeckId::A,
                stem,
                value,
            })?;
        }
        engine.dispatch(Command::PlayPause { deck: DeckId::A })?;
        let mut samples = Vec::with_capacity(length);
        while samples.len() < length {
            samples.extend(engine.render_offline(256));
        }
        samples.truncate(length);
        Ok(samples)
    };
    let dry = render(false, [1.; 4])?;
    let mut results = Vec::new();
    for (name, gains) in [
        ("unity", [1.; 4]),
        ("all-muted", [0.; 4]),
        ("vocals-muted", [0., 1., 1., 1.]),
        ("drums-muted", [1., 0., 1., 1.]),
        ("bass-muted", [1., 1., 1., 0.]),
        ("other-muted", [1., 1., 0., 1.]),
        ("vocals-half", [0.5, 1., 1., 1.]),
    ] {
        let samples = render(true, gains)?;
        assert!(samples.iter().all(|s| s.is_finite()));
        let max = samples
            .iter()
            .zip(&dry)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        let peak = samples.iter().map(|s| s.abs()).fold(0f32, f32::max);
        let rms = (samples.iter().map(|s| (*s as f64).powi(2)).sum::<f64>() / samples.len() as f64)
            .sqrt();
        if name == "unity" {
            assert_eq!(samples, dry);
        }
        if name == "all-muted" {
            let settled = samples
                .iter()
                .skip(44100 * 2)
                .map(|s| s.abs())
                .fold(0f32, f32::max);
            assert!(settled < 1e-7, "settled mute peak {settled}");
        }
        if name.ends_with("-muted") && name != "all-muted" {
            assert!(max > 1e-4);
        }
        let mut wav = hound::WavWriter::create(
            output.join(format!("{name}.wav")),
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )?;
        for sample in samples {
            wav.write_sample(sample)?;
        }
        wav.finalize()?;
        results
            .push(serde_json::json!({"case":name,"peak":peak,"rms":rms,"max_diff_from_dry":max}));
    }
    let summary =
        serde_json::json!({"frames":audio.frames,"source_rate":audio.sample_rate,"cases":results});
    std::fs::write(
        output.join("verification.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    println!("{summary}");
    Ok(())
}
