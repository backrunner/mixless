//! Opt-in real-library replay, including the preceding handoff's carried state.
use super::*;

#[test]
#[ignore = "uses an explicit real-library backup and local audio"]
fn audit_chained_playback() {
    let db = std::env::var("MIXLESS_SET_AUDIT_DB").expect("SQLite backup");
    let ids: Vec<TrackId> = std::env::var("MIXLESS_SET_AUDIT_TRACKS")
        .expect("previous,outgoing,incoming,following track IDs")
        .split(',')
        .map(|s| TrackId(s.trim().parse().unwrap()))
        .collect();
    assert_eq!(ids.len(), 4);
    let (dir, mut core, _) = super::super::tests::fixture();
    let c = Arc::get_mut(&mut core).unwrap();
    c.library = mixless_library::Library::open(std::path::Path::new(&db)).unwrap();
    if let Ok(cache) = std::env::var("MIXLESS_SET_AUDIT_STEMS") {
        c.stems = Some(mixless_stems::Processor::new(
            dir.path().join("models"),
            cache.into(),
            false,
        ));
    }
    load(&core, DeckId::A, ids[0], || true).unwrap();
    load(&core, DeckId::B, ids[1], || true).unwrap();
    let previous = crate::analysis::prepare(&core, ids[0]).unwrap();
    let outgoing = crate::analysis::prepare(&core, ids[1]).unwrap();
    let prior = pair::scheduled_pair(
        &core,
        &previous,
        &outgoing,
        1.,
        0.,
        PerformanceOffset::identity(),
        PerformanceOffset::identity(),
        Some(ids[2]),
        core.engine.four_stems_ready(DeckId::A) && core.engine.four_stems_ready(DeckId::B),
    )
    .unwrap()
    .plan;
    core.engine
        .cue_loaded_track(
            DeckId::A,
            ids[0],
            ((prior.t_in_a - 1.).max(0.) * previous.analysis.sample_rate as f32).round() as u64,
        )
        .unwrap();
    command(&core, Command::SetCrossfader { value: -1. }).unwrap();
    command(&core, Command::PlayPause { deck: DeckId::A }).unwrap();
    core.engine
        .load_plan_on((*prior).clone(), DeckId::A)
        .unwrap();
    core.engine
        .render_offline(((prior.duration_sec() + 3.) * 48_000.) as usize);
    let before = core.engine.snapshot();
    assert_eq!(before.automix_progress, 1.);
    assert!(before.decks[1].playing);
    let mut order = order::TrackOrder::new(ids.clone(), Some(ids[1]), 1);
    let (tx, _rx) = std::sync::mpsc::channel();
    let started = std::time::Instant::now();
    let next = stage(
        &core,
        DeckId::B,
        mixless_mixplan::established_entry(&prior, &outgoing.analysis),
        &mut order,
        ids.len(),
        false,
        || true,
        &tx,
    )
    .unwrap()
    .unwrap();
    let plan = next.transition.unwrap();
    assert_eq!(plan.summary.as_ref().unwrap().pair, (ids[1], ids[2]));
    let now = before.decks[1].frame as f32 / before.decks[1].src_sample_rate as f32;
    let samples = core.engine.render_offline(
        (((plan.t_out_a - now) / plan.outgoing_offset.rate + plan.duration_sec() + 2.) * 48_000.)
            as usize,
    );
    assert!(samples.iter().all(|s| s.is_finite() && s.abs() <= 1.));
    let done = core.engine.snapshot();
    assert_eq!(done.automix_progress, 1.);
    assert!(done.decks[0].playing);
    assert_eq!(done.decks[0].track_id, Some(ids[2]));
    println!(
        "Staged and rendered {:?} in {:?}: A {:.3}..{:.3}, B {:.3}..{:.3}, finite PCM",
        plan.summary,
        started.elapsed(),
        plan.t_in_a,
        plan.t_out_a,
        plan.t_in_b,
        plan.t_end_b
    );
    if let Ok(path) = std::env::var("MIXLESS_SET_AUDIT_PLANS") {
        std::fs::write(path, serde_json::to_vec_pretty(&[prior, plan]).unwrap()).unwrap();
    }
}
