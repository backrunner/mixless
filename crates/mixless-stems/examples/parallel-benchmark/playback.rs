//! Paced offline callback evidence, with actual cached stems and pitch shifting.
use mixless_engine::{AudioBuffer, Engine, EngineConfig};
use mixless_protocol::{Command, DeckId, StemKind, TrackId};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub struct Run {
    done: Arc<AtomicBool>,
    thread: Option<JoinHandle<Result<serde_json::Value, String>>>,
}

pub fn start(cache: PathBuf, seconds: f32) -> Result<Run, String> {
    let frames = (44100. * seconds) as usize;
    let pcm = (0..frames)
        .flat_map(|i| {
            let t = i as f32 / 44100.;
            let v = (t * std::f32::consts::TAU * 220.).sin() * 0.1
                + (t * std::f32::consts::TAU * 70.).sin() * 0.05;
            [v, v * 0.8]
        })
        .collect();
    let audio = Arc::new(AudioBuffer {
        samples: pcm,
        frames: frames as u64,
        sample_rate: 44100,
        loudness: Default::default(),
    });
    let processor = mixless_stems::Processor::new(PathBuf::new(), cache, false);
    let stems = processor
        .playback(
            &format!("parallel-benchmark-0-{seconds}"),
            frames as u64,
            44100,
        )
        .map_err(|e| e.to_string())?
        .ok_or("missing playback reference")?;
    let engine = Engine::new(EngineConfig {
        offline: true,
        sample_rate: 48000,
        block_frames: 128,
    })
    .map_err(|e| e.to_string())?;
    engine
        .load_buffer_if(
            DeckId::A,
            TrackId(1),
            audio.clone(),
            Arc::new(mixless_engine::compute_preview_waveform(&audio, 32)),
            "concurrent stem playback".into(),
            "".into(),
            || true,
        )
        .map_err(|e| e.to_string())?;
    if !engine
        .attach_stems(DeckId::A, TrackId(1), &audio, stems)
        .map_err(|e| e.to_string())?
    {
        return Err("stem attachment failed".into());
    }
    engine.set_bpm(DeckId::A, 128.);
    for command in [
        Command::SetChannelFader {
            deck: DeckId::A,
            value: 1.,
        },
        Command::SetCrossfader { value: -1. },
        Command::SetKeyLock {
            deck: DeckId::A,
            on: true,
        },
        Command::SetPitchSemitones {
            deck: DeckId::A,
            semitones: 2.,
        },
        Command::SetStemGain {
            deck: DeckId::A,
            stem: StemKind::Vocals,
            value: 0.5,
        },
        Command::SetLoopBeats {
            deck: DeckId::A,
            beats: 4.,
            on: true,
        },
        Command::PlayPause { deck: DeckId::A },
    ] {
        engine.dispatch(command).map_err(|e| e.to_string())?;
    }
    // Measure contention on an already running deck, not first-use stretcher
    // initialization before the first audible callback.
    for _ in 0..128 {
        engine.render_offline(128);
    }
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    let thread = std::thread::spawn(move || {
        let period = Duration::from_secs_f64(128. / 48000.);
        let mut next = Instant::now();
        let mut times = Vec::new();
        let mut peak = 0f32;
        while !stop.load(Ordering::Acquire) {
            let start = Instant::now();
            let block = engine.render_offline(128);
            times.push(start.elapsed().as_secs_f64() * 1000.);
            for value in block {
                if !value.is_finite() || value.abs() > 1. {
                    return Err("invalid rendered sample".into());
                }
                peak = peak.max(value.abs());
            }
            next += period;
            if let Some(wait) = next.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            } else {
                next = Instant::now();
            }
        }
        times.sort_by(f64::total_cmp);
        if times.is_empty()
            || peak <= 1e-5
            || !engine.snapshot().deck(DeckId::A).playing
            || !engine.stems_ready(DeckId::A)
        {
            return Err("playback was not exercised".into());
        }
        let p99 = times[times.len() * 99 / 100];
        let missed = times
            .iter()
            .filter(|ms| **ms >= period.as_secs_f64() * 1000.)
            .count();
        if p99 >= period.as_secs_f64() * 1000. {
            return Err(format!("callback p99 {p99} exceeded deadline"));
        }
        Ok(
            serde_json::json!({"blocks":times.len(),"p99_ms":p99,"max_ms":times.last(),"over_deadline":missed,"warmup_blocks":128,"deadline_ms":period.as_secs_f64()*1000.,"peak":peak,"stems":true,"pitch_semitones":2.}),
        )
    });
    Ok(Run {
        done,
        thread: Some(thread),
    })
}

impl Run {
    pub fn finish(mut self) -> Result<serde_json::Value, String> {
        self.done.store(true, Ordering::Release);
        self.thread
            .take()
            .unwrap()
            .join()
            .map_err(|_| "render worker panicked")?
    }
}

impl Drop for Run {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
