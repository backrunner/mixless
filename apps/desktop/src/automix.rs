//! Continuous host orchestration; sample-accurate cue, filter and fader motion
//! stays in the engine. Offline analysis and adjacent plans are prepared early.
mod order;
mod preparation;
pub use preparation::{Preparation, prepare_playlist};

use crate::state::AppCore;
use mixless_protocol::{Command, DeckId, PerformanceOffset, TrackId};
use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc::Sender};
use std::time::Duration;

pub enum AutomixMsg {
    Status(String),
    Done(Result<(), String>),
}

fn guarded(core: &AppCore, active: impl Fn() -> bool, action: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let _commit = core.automix_commit.lock().map_err(|_| "Automix commit lock poisoned")?;
    if active() { action()?; }
    Ok(())
}

fn command(core: &AppCore, cmd: Command) -> Result<(), String> {
    core.engine.dispatch(cmd).map_err(|e| e.to_string())
}

fn load(core: &AppCore, deck: DeckId, id: TrackId, active: impl Fn() -> bool) -> Result<(), String> {
    if !crate::analysis::load(core, deck, id, &active)? { return Ok(()); }
    guarded(core, active, || {
        for cmd in [
            Command::SetRate { deck, rate: 1. },
            Command::SetPitchSemitones { deck, semitones: 0. },
            Command::SetChannelGain { deck, db: 0. },
            Command::SetChannelFader { deck, value: 0.8 },
            Command::SetChannelFilter { deck, amount: 0. },
            Command::SetFxSend { deck, value: 0. },
            Command::SetKeyLock { deck, on: true },
            Command::SetReverse { deck, on: false },
            Command::SetLoopBeats { deck, beats: 4., on: false },
        ] { command(core, cmd)?; }
        for band in [mixless_protocol::EqBand::Low, mixless_protocol::EqBand::Mid, mixless_protocol::EqBand::High] {
            command(core, Command::SetEq { deck, band, db: 0. })?;
            command(core, Command::SetEqKill { deck, band, on: false })?;
        }
        Ok(())
    })
}

fn cue(core: &AppCore, deck: DeckId, track: &crate::analysis::PreparedTrack) -> Result<(), String> {
    let seconds = track.analysis.sections.iter().find(|s| s.label != mixless_protocol::SectionLabel::Silence)
        .map_or(0., |s| s.start_sec);
    let hard_in = core.library.cues(track.track.id).map_err(|e| e.to_string())?.into_iter()
        .filter(|c| c.user_set && c.kind == mixless_protocol::CueKind::In).map(|c| c.frame).min();
    let frame = hard_in.unwrap_or((seconds * track.analysis.sample_rate as f32).round() as u64);
    core.engine.cue_loaded_track(deck, track.track.id, frame).map_err(|e| e.to_string())
}

fn offset(d: &mixless_protocol::DeckSnapshot) -> PerformanceOffset {
    PerformanceOffset { rate: d.rate,
        pitch_semitones: d.pitch_semitones + if d.keylock { 0. } else { 12. * d.rate.log2() } }
}

/// When AUTO is enabled during a manual overlap, gently finish that overlap
/// before using the free deck. Disabling AUTO itself preserves both transports.
fn settle_overlap(core: &AppCore, outgoing: DeckId, active: impl Fn() -> bool) -> Result<(), String> {
    let snapshot = core.engine.snapshot();
    let incoming = if outgoing == DeckId::A { DeckId::B } else { DeckId::A };

    let from = if snapshot.xf_reverse { -snapshot.xfader } else { snapshot.xfader };
    let target = if outgoing == DeckId::A { -1. } else { 1. };
    guarded(core, &active, || {
        command(core, Command::SetXfReverse { on: false })?;
        command(core, Command::SetXfCurve { curve: mixless_protocol::XfCurve::EqualPower })
    })?;
    let d = snapshot.deck(outgoing);
    let remaining = d.frames.saturating_sub(d.frame) as f32 / d.src_sample_rate.max(1) as f32 / d.rate.max(0.01);
    let steps = (remaining * 0.2 * 100.).clamp(1., 30.) as u32;
    for step in 1..=steps {
        if !active() { return Ok(()); }
        let t = step as f32 / steps as f32;
        let t = t * t * (3. - 2. * t);
        guarded(core, &active, || command(core, Command::SetCrossfader { value: from + (target - from) * t }))?;
        std::thread::sleep(Duration::from_millis(10));
    }
    guarded(core, active, || {
        if core.engine.snapshot().deck(incoming).playing {
            command(core, Command::PlayPause { deck: incoming })?;
        }
        Ok(())
    })
}

pub fn run(core: Arc<AppCore>, tracks: Vec<TrackId>, shuffle: Arc<AtomicBool>, epoch: Arc<AtomicU64>, generation: u64, tx: Sender<AutomixMsg>) {
    let active = || epoch.load(Ordering::Acquire) == generation;
    let result = (|| -> Result<(), String> {
        if tracks.is_empty() { return Err("Import a track to start Automix".into()); }
        let initial = core.engine.snapshot();
        let x = if initial.xf_reverse { -initial.xfader } else { initial.xfader };
        let gain = |index: usize| {
            let d = &initial.decks[index];
            let cross = if index == 0 { 1. - x } else { 1. + x };
            cross * d.fader * 10f32.powf(d.gain_db / 20.)
        };
        let mut outgoing = initial.decks.iter().enumerate().filter(|(_, d)| d.playing)
            .max_by(|(a,_),(b,_)| gain(*a).total_cmp(&gain(*b)))
            .map(|(i,_)| DeckId::from_index(i).unwrap()).unwrap_or(DeckId::A);
        let mut order = order::TrackOrder::new(tracks.clone(), initial.deck(outgoing).playing.then_some(initial.deck(outgoing).track_id).flatten(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos() as u64);
        if initial.deck(outgoing).playing {
            settle_overlap(&core, outgoing, &active)?;
        } else {
            let mut started = false;
            for _ in 0..tracks.len() {
                if !active() { return Ok(()); }
                let id = order.next(shuffle.load(Ordering::Relaxed));
                if load(&core, outgoing, id, &active).is_err() { continue; }
                let prepared = crate::analysis::prepare(&core, id)?;
                guarded(&core, &active, || {
                    cue(&core, outgoing, &prepared)?;
                    command(&core, Command::SetCrossfader { value: -1. })?;
                    command(&core, Command::SetXfReverse { on: false })?;
                    command(&core, Command::PlayPause { deck: outgoing })
                })?;
                started = true;
                break;
            }
            if !started { return Err("No playable tracks in this playlist".into()); }
        }
        while active() {
            let incoming = if outgoing == DeckId::A { DeckId::B } else { DeckId::A };
            let mut loaded = false;
            let mut last_error = String::new();
            for _ in 0..tracks.len() {
                let id = order.next(shuffle.load(Ordering::Relaxed));
                if !active() { return Ok(()); }
                let title = core.library.get_track(id).map(|t| t.title).unwrap_or_default();
                let _ = tx.send(AutomixMsg::Status(format!("Preparing {title}")));
                match load(&core, incoming, id, &active) {
                    Ok(()) => { loaded = true; break; }
                    Err(error) => { last_error = error; }
                }
            }
            if !loaded { return Err(format!("No playable next track: {last_error}")); }
            if !active() { return Ok(()); }
            let a_id = core.engine.snapshot().deck(outgoing).track_id.ok_or("Deck is empty")?;
            let b_id = core.engine.snapshot().deck(incoming).track_id.ok_or("Deck is empty")?;
            let a = crate::analysis::prepare(&core, a_id)?;
            let b = crate::analysis::prepare(&core, b_id)?;
            let snapshot = core.engine.snapshot();
            let d = snapshot.deck(outgoing);
            if !d.playing && d.frame >= d.frames.saturating_sub(1) {
                // A cold load may finish after EOF. Continue with the prepared
                // track instead of leaving AUTO stranded on an ended deck.
                guarded(&core, &active, || {
                    cue(&core, incoming, &b)?;
                    command(&core, Command::SetCrossfader { value: if incoming == DeckId::A { -1. } else { 1. } })?;
                    command(&core, Command::PlayPause { deck: incoming })
                })?;
                outgoing = incoming;
                continue;
            }
            let now = d.frame as f32 / d.src_sample_rate.max(1) as f32;
            let lead = ((a.analysis.duration_sec - now) * 0.1).clamp(0.005, 0.25);
            let plan = preparation::pair(&core, &a, &b, now + lead, offset(d), offset(snapshot.deck(incoming)))?;
            let start = plan.t_in_a;
            let prepared = core.engine.prepare_plan_on(plan, outgoing).map_err(|e| e.to_string())?;
            guarded(&core, &active, || {
                if !core.engine.snapshot().deck(outgoing).playing { return Err("Playback stopped".into()); }
                command(&core, Command::SetLoopBeats { deck: outgoing, beats: d.loop_beats, on: false })?;
                // Bring a muted, manually loaded outgoing channel into the mix.
                if d.fader <= 0.001 { command(&core, Command::SetChannelFader { deck: outgoing, value: 0.8 })?; }
                core.engine.commit_plan(prepared).map_err(|e| e.to_string())
            })?;
            // Look one track ahead while the staged pair plays. Decode is
            // bounded by the shared PCM cache, independent of the UI clock.
            let mut lookahead = order.clone();
            let next = lookahead.next(shuffle.load(Ordering::Relaxed));
            let warm_core = core.clone();
            std::thread::spawn(move || {
                if let Ok(track) = crate::analysis::prepare(&warm_core, next) {
                    let _ = crate::analysis::decode(&warm_core, &track);
                }
            });
            let mut phase = String::new();
            loop {
                if !active() { return Ok(()); }
                let snapshot = core.engine.snapshot();
                if !snapshot.automix_on {
                    if snapshot.automix_progress < 1.0 { return Err("Automix stopped after a playback change".into()); }
                    break;
                }
                let seconds = snapshot.deck(outgoing).frame as f32 / snapshot.deck(outgoing).src_sample_rate.max(1) as f32;
                let next_phase = if snapshot.automix_paused { "Paused".into() }
                    else if seconds < start && snapshot.automix_progress == 0. { format!("Cueing {}", b.track.title) }
                    else { format!("Mixing deck {} into {}", if outgoing == DeckId::A { "A" } else { "B" }, if incoming == DeckId::A { "A" } else { "B" }) };
                if next_phase != phase { phase = next_phase; let _ = tx.send(AutomixMsg::Status(phase.clone())); }
                std::thread::sleep(Duration::from_millis(25));
            }
            outgoing = incoming;
        }
        Ok(())
    })();
    if active() { let _ = tx.send(AutomixMsg::Done(result)); }
}

#[cfg(test)]
mod tests;
