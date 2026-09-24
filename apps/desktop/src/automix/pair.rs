//! Plan and cache a pair for its actual entry and carried performance state.
use super::preparation::ScheduledPair;
use super::*;

#[cfg(test)]
pub(super) fn pair(
    core: &AppCore,
    a: &crate::analysis::PreparedTrack,
    b: &crate::analysis::PreparedTrack,
    earliest: f32,
    offset_a: PerformanceOffset,
    offset_b: PerformanceOffset,
) -> Result<mixless_protocol::MixPlan, String> {
    let tracks = core.mix_preparation.tracks();
    let following = tracks
        .iter()
        .position(|id| *id == b.track.id)
        .and_then(|i| tracks.get((i + 1) % tracks.len()).copied());
    scheduled_pair(
        core, a, b, earliest, 0., offset_a, offset_b, following, false,
    )
    .map(|p| (*p.plan).clone())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn scheduled_pair(
    core: &AppCore,
    a: &crate::analysis::PreparedTrack,
    b: &crate::analysis::PreparedTrack,
    earliest: f32,
    entry: f32,
    offset_a: PerformanceOffset,
    offset_b: PerformanceOffset,
    following: Option<TrackId>,
    stem_playback: bool,
) -> Result<ScheduledPair, String> {
    // Track-level IN/OUT markers constrain the corresponding side only.
    let ca: Vec<_> = core
        .library
        .cues(a.track.id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|c| c.kind != mixless_protocol::CueKind::In)
        .collect();
    let cb: Vec<_> = core
        .library
        .cues(b.track.id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|c| c.kind != mixless_protocol::CueKind::Out)
        .collect();
    // Read only current-version cached evidence. Do not stall playback for a
    // third decode/model pass; the playlist preparation queue warms this cache.
    let following_analysis = following.and_then(|id| {
        core.library
            .load_analysis(id, mixless_analyze::ANALYSIS_VERSION)
            .ok()
            .flatten()
    });
    let live_moves = core.settings.get().live_moves;
    let key = format!(
        "{}:{}:{}:{}:{}:{}:{}:{:?}:{:?}:{}:{}:{:?}:{:?}:{}:{:?}",
        entry.to_bits(),
        evidence_key(&a.analysis)?,
        evidence_key(&b.analysis)?,
        a.track.id.0,
        b.track.id.0,
        a.track.content_hash,
        b.track.content_hash,
        offset_a,
        offset_b,
        serde_json::to_string(&ca).map_err(|e| e.to_string())?,
        serde_json::to_string(&cb).map_err(|e| e.to_string())?,
        following,
        following_analysis
            .as_ref()
            .map(|a| (&a.camelot, a.key_confidence, a.tempo.global_bpm)),
        stem_playback,
        live_moves,
    );
    if let Some(pair) = core.mix_preparation.scheduled(&key, earliest) {
        return Ok(pair);
    }
    let cached = core
        .mix_preparation
        .plans
        .lock()
        .expect("plans")
        .get(&key)
        .cloned();
    let base = cached.unwrap_or_else(|| {
        let plan = choose_plan(
            a,
            b,
            &ca,
            &cb,
            offset_a,
            offset_b,
            0.,
            entry,
            false,
            following_analysis.as_ref(),
            stem_playback,
            live_moves,
        );
        let mut plans = core.mix_preparation.plans.lock().expect("plans");
        let mut order = core.mix_preparation.order.lock().expect("plan order");
        if !plans.contains_key(&key) {
            order.push_back(key.clone());
        }
        plans.insert(key.clone(), plan.clone());
        while order.len() > 256 {
            if let Some(old) = order.pop_front() {
                plans.remove(&old);
            }
        }
        plan
    });
    if base.failure_reason.is_none() && base.t_in_a >= earliest {
        return Ok(ScheduledPair {
            key,
            plan: Arc::new(base),
        });
    }
    let plan = choose_plan(
        a,
        b,
        &ca,
        &cb,
        offset_a,
        offset_b,
        earliest,
        entry,
        true,
        following_analysis.as_ref(),
        stem_playback,
        live_moves,
    );
    if let Some(error) = &plan.failure_reason {
        return Err(error.clone());
    }
    Ok(ScheduledPair {
        key,
        plan: Arc::new(plan),
    })
}

fn choose_plan(
    a: &crate::analysis::PreparedTrack,
    b: &crate::analysis::PreparedTrack,
    ca: &[mixless_protocol::Cue],
    cb: &[mixless_protocol::Cue],
    offset_a: PerformanceOffset,
    offset_b: PerformanceOffset,
    earliest: f32,
    entry: f32,
    fallback: bool,
    following: Option<&mixless_protocol::TrackAnalysis>,
    stem_playback: bool,
    live_moves: mixless_protocol::LiveMoves,
) -> mixless_protocol::MixPlan {
    let planner = mixless_mixplan::Planner::with_options(mixless_mixplan::PlannerOptions {
        harmonic_key_shift: true,
        earliest_outgoing_sec: earliest,
        outgoing_entry_sec: entry,
        stem_playback,
        live_moves,
        ..Default::default()
    });
    let mut best: Option<mixless_protocol::MixPlan> = None;
    // A marked exit already behind the playhead cannot be used on this pass.
    // Keep the analyzed future windows available when AUTO is enabled late.
    let future: Vec<_> = ca
        .iter()
        .filter(|c| {
            !(c.user_set
                && c.kind == mixless_protocol::CueKind::Out
                && c.frame as f32 / a.analysis.sample_rate.max(1) as f32 <= earliest)
        })
        .cloned()
        .collect();
    let outgoing = mixless_mixplan::cue_policy::cue_choices(&future, true);
    let incoming = mixless_mixplan::cue_policy::cue_choices(cb, false);
    for ca in &outgoing {
        for cb in &incoming {
            let ctx = mixless_mixplan::PlanContext {
                outgoing: &a.analysis,
                incoming: &b.analysis,
                cues_out: ca,
                cues_in: cb,
                offset_a,
                offset_b,
            };
            let mut plan = planner.plan_with_following(&ctx, following);
            if fallback && plan.failure_reason.is_some() {
                plan = mixless_mixplan::short_handoff(&ctx, earliest);
            }
            let score = |p: &mixless_protocol::MixPlan| {
                if p.failure_reason.is_some() {
                    -1.
                } else {
                    p.summary.as_ref().map_or(0., |s| s.score)
                }
            };
            if best.as_ref().is_none_or(|old| score(&plan) > score(old)) {
                best = Some(plan);
            }
        }
    }
    best.unwrap()
}

// Key actual evidence, not a global analysis counter: an unrelated background
// completion must not discard this set's decisions. Stream into a hash so full
// moment/stem payloads are not retained or copied into the plan cache key.
fn evidence_key(analysis: &mixless_protocol::TrackAnalysis) -> Result<u64, String> {
    use std::hash::Hasher;
    struct Sink(std::collections::hash_map::DefaultHasher);
    impl std::io::Write for Sink {
        fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
            self.0.write(data);
            Ok(data.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sink = Sink(Default::default());
    serde_json::to_writer(&mut sink, analysis).map_err(|e| e.to_string())?;
    Ok(sink.0.finish())
}

#[cfg(test)]
mod audit {
    use super::*;

    #[test]
    #[ignore = "uses an explicit real-library backup"]
    fn audit_cross_playlist_entries() {
        let db = std::env::var("MIXLESS_SET_AUDIT_DB").expect("SQLite backup");
        let (_dir, mut core, _) = super::super::tests::fixture();
        let c = Arc::get_mut(&mut core).unwrap();
        c.library = mixless_library::Library::open(std::path::Path::new(&db)).unwrap();
        let source = core
            .library
            .playlist_tracks(mixless_protocol::PlaylistId(4))
            .unwrap();
        for destination in [2, 3] {
            let target = core
                .library
                .playlist_tracks(mixless_protocol::PlaylistId(destination))
                .unwrap();
            let b = crate::analysis::prepare(&core, target[0].id).unwrap();
            for a in source.iter().take(5) {
                let a = crate::analysis::prepare(&core, a.id).unwrap();
                let p = scheduled_pair(
                    &core,
                    &a,
                    &b,
                    22.,
                    0.,
                    PerformanceOffset::identity(),
                    PerformanceOffset::identity(),
                    Some(target[1].id),
                    true,
                )
                .unwrap()
                .plan;
                println!(
                    "{} -> {}: {:?}, A {:.3}..{:.3}, B {:.3}..{:.3}, fallback {}",
                    a.track.title,
                    b.track.title,
                    p.summary.as_ref().map(|s| s.strategy),
                    p.t_in_a,
                    p.t_out_a,
                    p.t_in_b,
                    p.t_end_b,
                    p.summary.as_ref().is_some_and(|s| s.used_fallback)
                );
            }
        }
    }
}
