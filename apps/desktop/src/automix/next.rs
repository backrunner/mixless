//! Stage a usable pair; a decoded but unmixable candidate must not stop the queue.
use super::*;

pub(super) struct Next {
    pub incoming: crate::analysis::PreparedTrack,
    pub transition: Option<Arc<mixless_protocol::MixPlan>>,
}

pub(super) fn stage(
    core: &Arc<AppCore>,
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
                            value: 1.,
                        },
                    )
                })?;
            }
            let mut lookahead = order.clone();
            let following = lookahead.next(shuffle.load(Ordering::Relaxed));
            let transition = prepare_and_publish(core, outgoing, &active, || {
                let snap = core.engine.snapshot();
                let d = snap.deck(outgoing);
                if !d.playing && d.frame >= d.frames.saturating_sub(1) {
                    return Ok(None);
                }
                if !d.playing {
                    return Err("Playback stopped".into());
                }
                let now = d.frame as f32 / d.src_sample_rate.max(1) as f32;
                let lead = ((a.analysis.duration_sec - now) * 0.1).clamp(0.005, 0.75);
                let stem_playback =
                    core.engine.stems_ready(outgoing) && core.engine.stems_ready(incoming);
                let plan = preparation::pair_from_entry(
                    core,
                    &a,
                    &b,
                    now + lead,
                    played_from,
                    offset(d),
                    offset(snap.deck(incoming)),
                    Some(following),
                    stem_playback,
                )?;
                let display = Arc::new(plan.clone());
                let prepared = core
                    .engine
                    .prepare_plan_on(plan, outgoing)
                    .map_err(|e| e.to_string())?;
                Ok(Some((display, prepared)))
            })?;
            if !active() {
                return Ok(None);
            }
            Ok(Some(Next {
                incoming: b,
                transition,
            }))
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

// Preparing can take longer than the remaining lead time. Publication belongs
// to the same bounded retry as planning, so a missed start replans this pair
// instead of stopping AUTO or consuming the next playlist entry.
fn prepare_and_publish(
    core: &Arc<AppCore>,
    outgoing: DeckId,
    active: impl Fn() -> bool,
    mut prepare: impl FnMut() -> Result<
        Option<(Arc<mixless_protocol::MixPlan>, mixless_engine::PreparedMix)>,
        String,
    >,
) -> Result<Option<Arc<mixless_protocol::MixPlan>>, String> {
    let mut last_error = String::new();
    for _ in 0..3 {
        if !active() {
            return Ok(None);
        }
        let attempt = (|| {
            let Some((plan, prepared)) = prepare()? else {
                return Ok(None);
            };
            guarded(core, &active, || {
                let d = core.engine.snapshot().deck(outgoing).clone();
                if !d.playing {
                    return Err("Playback stopped".into());
                }
                command(
                    core,
                    Command::SetLoopBeats {
                        deck: outgoing,
                        beats: d.loop_beats,
                        on: false,
                    },
                )?;
                core.engine.commit_plan(prepared).map_err(|e| e.to_string())
            })?;
            Ok(active().then_some(plan))
        })();
        match attempt {
            Ok(plan) => return Ok(plan),
            Err(error) => last_error = error,
        }
    }
    Err(last_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missed_publication_replans_the_same_pair_and_completes() {
        let (_dir, core, tracks) = super::super::tests::fixture();
        load(&core, DeckId::A, tracks[0], &|| true).unwrap();
        load(&core, DeckId::B, tracks[1], &|| true).unwrap();
        let a = crate::analysis::prepare(&core, tracks[0]).unwrap();
        let b = crate::analysis::prepare(&core, tracks[1]).unwrap();
        core.engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        let mut attempts = 0;
        let plan = prepare_and_publish(
            &core,
            DeckId::A,
            || true,
            || {
                attempts += 1;
                let now = core.engine.snapshot().decks[0].frame as f32 / 48_000.;
                let context = mixless_mixplan::PlanContext {
                    outgoing: &a.analysis,
                    incoming: &b.analysis,
                    cues_out: &[],
                    cues_in: &[],
                    offset_a: PerformanceOffset::identity(),
                    offset_b: PerformanceOffset::identity(),
                };
                let plan = mixless_mixplan::short_handoff(&context, now + 0.5);
                assert!(plan.failure_reason.is_none(), "{plan:?}");
                let prepared = core
                    .engine
                    .prepare_plan_on(plan.clone(), DeckId::A)
                    .unwrap();
                if attempts == 1 {
                    // Deterministically pass the first launch while the host holds
                    // a prepared plan, reproducing a delayed publication.
                    core.engine
                        .render_offline(((plan.t_in_a - now + 0.1) * 48_000.) as usize);
                }
                Ok(Some((Arc::new(plan), prepared)))
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(attempts, 2);
        assert_eq!(plan.summary.as_ref().unwrap().pair, (tracks[0], tracks[1]));
        assert!(core.engine.snapshot().automix_on);
        core.engine.render_offline(5 * 48_000);
        let snapshot = core.engine.snapshot();
        assert_eq!(snapshot.automix_progress, 1.);
        assert!(snapshot.decks[1].playing);
    }
}
