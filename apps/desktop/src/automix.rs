//! Host orchestration: only the next pair is decoded/planned. Audio timing
//! stays in the engine, independent of UI repaint or this worker's polling.
use crate::state::AppCore;
use mixless_protocol::{Command, DeckId, PerformanceOffset, TrackAnalysis, TrackId};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    mpsc::Sender,
    Arc,
};
use std::time::Duration;

pub enum AutomixMsg {
    Status(String),
    Done(Result<(), String>),
}
fn load(
    core: &AppCore,
    deck: DeckId,
    id: TrackId,
    active: impl Fn() -> bool,
) -> Result<(), String> {
    let _load = core
        .deck_load
        .lock()
        .map_err(|_| "Deck load mutex poisoned")?;
    if !active() {
        return Ok(());
    }
    let track = core.library.get_track(id).map_err(|e| e.to_string())?;
    if !core
        .engine
        .load_file_if(
            deck,
            id,
            std::path::Path::new(&track.path),
            track.title,
            track.artist,
            &active,
        )
        .map_err(|e| e.to_string())?
    {
        return Ok(());
    }
    if !active() {
        return Ok(());
    }
    core.engine.set_bpm(deck, track.bpm.unwrap_or(120.));
    for cue in core.library.cues(id).map_err(|e| e.to_string())? {
        core.engine.set_cue_frame(deck, cue.index, cue.frame);
    }
    for cmd in [
        Command::SetRate { deck, rate: 1. },
        Command::SetPitchSemitones {
            deck,
            semitones: 0.,
        },
        Command::SetChannelGain { deck, db: 0. },
        Command::SetChannelFader { deck, value: 0.8 },
        Command::SetChannelFilter { deck, amount: 0. },
        Command::SetFxSend { deck, value: 0. },
        Command::SetKeyLock { deck, on: true },
        Command::SetReverse { deck, on: false },
    ] {
        core.engine.dispatch(cmd).map_err(|e| e.to_string())?;
    }
    for band in [
        mixless_protocol::EqBand::Low,
        mixless_protocol::EqBand::Mid,
        mixless_protocol::EqBand::High,
    ] {
        core.engine
            .dispatch(Command::SetEq { deck, band, db: 0. })
            .map_err(|e| e.to_string())?;
        core.engine
            .dispatch(Command::SetEqKill {
                deck,
                band,
                on: false,
            })
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn analysis(core: &AppCore, deck: DeckId) -> Result<TrackAnalysis, String> {
    let snapshot = core.engine.snapshot();
    let d = snapshot.deck(deck);
    let id = d.track_id.ok_or("Automix deck was unloaded")?;
    let track = core.library.get_track(id).map_err(|e| e.to_string())?;
    let hash = mixless_library::content_hash(std::path::Path::new(&track.path))
        .map_err(|e| e.to_string())?;
    if hash == track.content_hash {
        if let Some(cached) = core
            .library
            .load_analysis(id, mixless_analyze::ANALYSIS_VERSION)
            .map_err(|e| e.to_string())?
        {
            if cached.sample_rate == d.src_sample_rate
                && (cached.duration_sec - d.frames as f32 / d.src_sample_rate as f32).abs() < 0.02
            {
                core.engine.set_bpm(deck, cached.tempo.global_bpm);
                return Ok(cached);
            }
        }
    }
    let analysis = core
        .analyzer
        .analyze_track(id, std::path::Path::new(&track.path))
        .map_err(|e| e.to_string())?;
    // Stale library revisions may still be played, but their results cannot
    // poison the cache for the previously imported file.
    if hash == track.content_hash {
        core.library
            .save_analysis(&analysis, &hash, mixless_analyze::ANALYSIS_VERSION)
            .map_err(|e| e.to_string())?;
        core.library
            .set_analysis_meta(
                id,
                Some(analysis.tempo.global_bpm),
                analysis.key.as_deref(),
                analysis.camelot.as_deref(),
            )
            .map_err(|e| e.to_string())?;
    }
    core.engine.set_bpm(deck, analysis.tempo.global_bpm);
    Ok(analysis)
}

fn offset(d: &mixless_protocol::DeckSnapshot) -> PerformanceOffset {
    PerformanceOffset {
        rate: d.rate,
        pitch_semitones: d.pitch_semitones + if d.keylock { 0. } else { 12. * d.rate.log2() },
    }
}

pub fn run(
    core: Arc<AppCore>,
    tracks: Vec<TrackId>,
    epoch: Arc<AtomicU64>,
    generation: u64,
    tx: Sender<AutomixMsg>,
) {
    let active = || epoch.load(Ordering::Acquire) == generation;
    let result = (|| -> Result<(), String> {
        if tracks.len() < 2 {
            return Err("Select a playlist with at least two local tracks".into());
        }
        let initial = core.engine.snapshot();
        if initial.decks.iter().filter(|d| d.playing).count() > 1 {
            return Err("Pause the incoming deck before starting AUTO".into());
        }
        let (mut outgoing, mut position) =
            if let Some((index, d)) = initial.decks.iter().enumerate().find(|(_, d)| d.playing) {
                let position = tracks
                    .iter()
                    .position(|id| Some(*id) == d.track_id)
                    .ok_or("Playing track is not in the selected playlist")?;
                (DeckId::from_index(index).unwrap(), position)
            } else {
                if !active() {
                    return Ok(());
                }
                load(&core, DeckId::A, tracks[0], active)?;
                if !active() {
                    return Ok(());
                }
                core.engine
                    .dispatch(Command::SetCrossfader { value: -1. })
                    .map_err(|e| e.to_string())?;
                core.engine
                    .dispatch(Command::PlayPause { deck: DeckId::A })
                    .map_err(|e| e.to_string())?;
                (DeckId::A, 0)
            };
        while position + 1 < tracks.len() && active() {
            let incoming = if outgoing == DeckId::A {
                DeckId::B
            } else {
                DeckId::A
            };
            let _ = tx.send(AutomixMsg::Status(format!(
                "Preparing {} / {}",
                position + 2,
                tracks.len()
            )));
            load(&core, incoming, tracks[position + 1], active)?;
            if !active() {
                return Ok(());
            }
            let a = analysis(&core, outgoing)?;
            let b = analysis(&core, incoming)?;
            let ca = core.library.cues(a.track_id).map_err(|e| e.to_string())?;
            let cb = core.library.cues(b.track_id).map_err(|e| e.to_string())?;
            let snapshot = core.engine.snapshot();
            let d = snapshot.deck(outgoing);
            let planner = mixless_mixplan::Planner::with_options(mixless_mixplan::PlannerOptions {
                earliest_outgoing_sec: d.frame as f32 / d.src_sample_rate as f32 + 1.0,
                ..Default::default()
            });
            let plan =
                planner.plan_pair(&a, &b, &ca, &cb, offset(d), offset(snapshot.deck(incoming)));
            if let Some(reason) = plan.failure_reason.as_ref() {
                return Err(reason.clone());
            }
            let summary = plan
                .summary
                .as_ref()
                .ok_or("No automix transition available")?;
            let status = format!(
                "{:?} · {} bars{}",
                summary.strategy,
                summary.length_bars,
                if summary.used_fallback {
                    " · fallback"
                } else {
                    ""
                }
            );
            if !active() {
                return Ok(());
            }
            core.engine
                .load_plan_on(plan, outgoing)
                .map_err(|e| e.to_string())?;
            let _ = tx.send(AutomixMsg::Status(status));
            loop {
                if !active() {
                    return Ok(());
                }
                let snapshot = core.engine.snapshot();
                if !snapshot.automix_on {
                    if snapshot.automix_progress < 1.0 {
                        return Err("Automix stopped after a transport or track change".into());
                    }
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            outgoing = incoming;
            position += 1;
        }
        Ok(())
    })();
    if active() {
        let _ = tx.send(AutomixMsg::Done(result));
    }
}
