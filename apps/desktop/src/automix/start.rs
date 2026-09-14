//! Start a playlist or recover from EOF with a short, beat-scaled level ride.
//! Progress follows rendered source frames, so a delayed callback cannot skip
//! the entrance. Engine smoothing handles the 100 Hz host control updates.
use super::*;

pub(super) fn fade_in(
    core: &AppCore,
    deck: DeckId,
    track: &crate::analysis::PreparedTrack,
    active: impl Fn() -> bool,
    tx: &Sender<AutomixMsg>,
) -> Result<(), String> {
    let id = track.track.id;
    let frame = cue_frame(core, track)?.min(
        ((track.analysis.duration_sec * track.analysis.sample_rate as f32) as u64)
            .saturating_sub(1),
    );
    let sr = track.analysis.sample_rate.max(1) as f32;
    let seconds = frame as f32 / sr;
    let bar = track
        .analysis
        .bars
        .iter()
        .find(|b| seconds >= b.start_sec && seconds < b.end_sec);
    // Leave an exposed voice clear; a percussive entrance can open a gentle LPF.
    let filter = if bar.is_some_and(|b| b.vocal_presence < 0.3 && b.kick_salience > 0.4) {
        -0.32
    } else {
        0.
    };
    let bpm = track
        .analysis
        .tempo
        .segments
        .first()
        .map_or(120., |s| s.bpm)
        .max(30.);
    let duration = (60. / bpm * 2.)
        .clamp(0.35, 1.6)
        .min((track.analysis.duration_sec - seconds).max(0.01) * 0.25);
    guarded(core, &active, || {
        command(
            core,
            Command::SetCrossfader {
                value: if deck == DeckId::A { -1. } else { 1. },
            },
        )?;
        command(core, Command::SetXfReverse { on: false })?;
        command(core, Command::SetChannelFader { deck, value: 0. })?;
        command(
            core,
            Command::SetChannelFilter {
                deck,
                amount: filter,
            },
        )?;
        core.engine
            .cue_loaded_track(deck, id, frame)
            .map_err(|e| e.to_string())?;
        command(core, Command::PlayPause { deck })
    })?;
    let _ = tx.send(AutomixMsg::Status(format!(
        "Starting {}",
        track.track.title
    )));
    let mut last = -1.;
    while active() {
        let d = core.engine.snapshot().deck(deck).clone();
        if d.track_id != Some(id) || !d.playing {
            return Ok(());
        }
        // The seek is consumed by the next callback. Do not advance the fade
        // from a stale pre-seek playhead.
        let t = (d.frame.saturating_sub(frame) as f32 / sr / duration).clamp(0., 1.);
        if t > last {
            let ease = t * t * t * (10. + t * (-15. + 6. * t));
            guarded(core, &active, || {
                command(
                    core,
                    Command::SetChannelFader {
                        deck,
                        value: ease,
                    },
                )?;
                command(
                    core,
                    Command::SetChannelFilter {
                        deck,
                        amount: filter * (1. - ease),
                    },
                )
            })?;
            last = t;
        }
        if t >= 1. {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
