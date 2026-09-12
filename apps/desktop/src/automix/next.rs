//! Stage a usable pair; a decoded but unmixable candidate must not stop the queue.
use super::*;

pub(super) struct Next {
    pub incoming: crate::analysis::PreparedTrack,
    pub transition: Option<(Arc<mixless_protocol::MixPlan>, mixless_engine::PreparedMix)>,
}

pub(super) fn stage(
    core: &AppCore,
    outgoing: DeckId,
    played_from: f32,
    order: &mut order::TrackOrder,
    count: usize,
    shuffle: &AtomicBool,
    active: impl Fn() -> bool,
    tx: &Sender<AutomixMsg>,
) -> Result<Option<Next>, String> {
    let incoming = if outgoing == DeckId::A {
        DeckId::B
    } else {
        DeckId::A
    };
    let Some(a_id) = core.engine.snapshot().deck(outgoing).track_id else {
        return Ok(None);
    };
    let a = crate::analysis::prepare(core, a_id)?;
    let mut last_error = String::new();
    for _ in 0..count {
        if !active() {
            return Ok(None);
        }
        let id = order.next(shuffle.load(Ordering::Relaxed));
        let title = core
            .library
            .get_track(id)
            .map(|t| t.title)
            .unwrap_or_default();
        let _ = tx.send(AutomixMsg::Preparing(title.clone()));
        let result = (|| {
            load(core, incoming, id, &active)?;
            if !active() {
                return Ok(None);
            }
            let b = crate::analysis::prepare(core, id)?;
            let d = core.engine.snapshot().deck(outgoing).clone();
            if !d.playing && d.frame >= d.frames.saturating_sub(1) {
                return Ok(Some(Next {
                    incoming: b,
                    transition: None,
                }));
            }
            if !d.playing {
                return Err("Playback stopped".into());
            }
            if d.fader <= 0.001 {
                guarded(core, &active, || {
                    command(
                        core,
                        Command::SetChannelFader {
                            deck: outgoing,
                            value: 0.8,
                        },
                    )
                })?;
            }
            let mut error = String::new();
            for _ in 0..3 {
                if !active() {
                    return Ok(None);
                }
                let snap = core.engine.snapshot();
                let d = snap.deck(outgoing);
                let now = d.frame as f32 / d.src_sample_rate.max(1) as f32;
                let lead = ((a.analysis.duration_sec - now) * 0.1).clamp(0.005, 0.75);
                let plan = preparation::pair_from_entry(
                    core,
                    &a,
                    &b,
                    now + lead,
                    played_from,
                    offset(d),
                    offset(snap.deck(incoming)),
                )?;
                let display = Arc::new(plan.clone());
                match core.engine.prepare_plan_on(plan, outgoing) {
                    Ok(prepared) => {
                        return Ok(Some(Next {
                            incoming: b,
                            transition: Some((display, prepared)),
                        }));
                    }
                    Err(e) => error = e.to_string(),
                }
            }
            Err(error)
        })();
        match result {
            Ok(next) => return Ok(next),
            Err(error) => {
                last_error = error;
                let _ = tx.send(AutomixMsg::Status(format!("Skipped {title}")));
            }
        }
    }
    Err(format!(
        "No usable transition in this playlist: {last_error}"
    ))
}
