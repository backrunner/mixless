//! Select the audible deck or load the first playable queue item, then start it.
use super::*;

pub(super) fn begin(
    core: &Arc<AppCore>,
    tracks: &[TrackId],
    active: impl Fn() -> bool,
    tx: &Sender<AutomixMsg>,
) -> Result<Option<(DeckId, order::TrackOrder, f32)>, String> {
    let initial = core.engine.snapshot();
    let x = if initial.xf_reverse {
        -initial.xfader
    } else {
        initial.xfader
    };
    let gain = |index: usize| {
        let d = &initial.decks[index];
        let cross = if index == 0 { 1. - x } else { 1. + x };
        cross * d.fader * 10f32.powf((d.gain_db + d.limiter_gain_db) / 20.)
    };
    let outgoing = initial
        .decks
        .iter()
        .enumerate()
        .filter(|(_, d)| {
            d.playing
                && d.track_id.is_some()
                && d.frames > 0
                && d.frame < d.frames.saturating_sub(1)
        })
        .max_by(|(a, _), (b, _)| gain(*a).total_cmp(&gain(*b)))
        .map(|(i, _)| DeckId::from_index(i).unwrap())
        .unwrap_or_else(|| {
            initial
                .decks
                .iter()
                .position(|d| {
                    d.track_id.is_some() && d.frames > 0 && d.frame < d.frames.saturating_sub(1)
                })
                .and_then(DeckId::from_index)
                .unwrap_or(DeckId::A)
        });
    let has_loaded = initial.deck(outgoing).track_id.is_some()
        && initial.deck(outgoing).frames > 0
        && initial.deck(outgoing).frame < initial.deck(outgoing).frames.saturating_sub(1);
    let has_playing = initial.deck(outgoing).playing
        && initial.deck(outgoing).track_id.is_some()
        && initial.deck(outgoing).frames > 0
        && initial.deck(outgoing).frame < initial.deck(outgoing).frames.saturating_sub(1);
    let mut order = order::TrackOrder::new(
        tracks.to_vec(),
        has_loaded
            .then_some(initial.deck(outgoing).track_id)
            .flatten(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64,
    );
    if has_playing {
        settle_overlap(&core, outgoing, &active)?;
    } else if has_loaded {
        let id = initial.deck(outgoing).track_id.unwrap();
        let prepared = crate::analysis::prepare(&core, id)?;
        start::fade_in(&core, outgoing, &prepared, &active, &tx)?;
    } else {
        let mut started = false;
        for _ in 0..tracks.len() {
            if !active() {
                return Ok(None);
            }
            let id = order.next(false);
            let title = core
                .library
                .get_track(id)
                .map(|t| t.title)
                .unwrap_or_default();
            let _ = tx.send(AutomixMsg::Preparing(title));
            let attempt = (|| {
                load(core, outgoing, id, &active)?;
                if !active() {
                    return Ok(());
                }
                let prepared = crate::analysis::prepare(core, id)?;
                start::fade_in(core, outgoing, &prepared, &active, tx)
            })();
            if attempt.is_err() {
                continue;
            }
            if !active() {
                return Ok(None);
            }
            started = true;
            break;
        }
        if !started {
            return Err("No playable tracks in this playlist".into());
        }
    }
    let played_from = if has_playing {
        0.
    } else {
        let d = core.engine.snapshot().deck(outgoing).clone();
        let t = crate::analysis::prepare(&core, d.track_id.ok_or("Deck unloaded")?)?;
        cue_frame(&core, &t)? as f32 / t.analysis.sample_rate.max(1) as f32
    };
    Ok(Some((outgoing, order, played_from)))
}
