use super::*;

#[test]
fn sequence_carries_entries_and_offsets_and_reuses_the_published_decisions() {
    let (_dir, core, tracks) = super::super::tests::fixture();
    *core.mix_preparation.playlist.lock().unwrap() = tracks.clone();
    let pairs = plan_sequence(&core, &tracks, &|| true).unwrap();
    assert!(pairs[..tracks.len() - 1].iter().all(Option::is_some));
    assert!(
        pairs.last().unwrap().is_none(),
        "a new lap is a separate occurrence"
    );
    for w in pairs[..tracks.len() - 1].windows(2) {
        let a = &w[0].as_ref().unwrap().plan;
        let b = &w[1].as_ref().unwrap().plan;
        assert!(b.t_in_a >= a.t_end_b + STAGING_SECONDS * a.incoming_offset_end.rate);
        assert_eq!(b.outgoing_offset, a.incoming_offset_end);
    }
    core.mix_preparation.sequence.lock().unwrap().pairs = pairs.clone();
    let again = plan_sequence(&core, &tracks, &|| true).unwrap();
    core.analysis
        .content_revision
        .fetch_add(1, Ordering::Release);
    let unchanged = plan_sequence(&core, &tracks, &|| true).unwrap();
    for (a, b) in pairs.iter().zip(&unchanged) {
        if let (Some(a), Some(b)) = (a, b) {
            assert!(Arc::ptr_eq(&a.plan, &b.plan));
        }
    }
    for (a, b) in pairs.iter().zip(&again) {
        if let (Some(a), Some(b)) = (a, b) {
            assert!(Arc::ptr_eq(&a.plan, &b.plan));
        }
    }
    let anchor = pairs[0].as_ref().unwrap().plan.clone();
    core.mix_preparation.sequence.lock().unwrap().anchor = Some(anchor.clone());
    let anchored = plan_sequence(&core, &tracks, &|| true).unwrap();
    assert!(Arc::ptr_eq(&anchored[0].as_ref().unwrap().plan, &anchor));
    assert!(Arc::ptr_eq(
        &anchored[1].as_ref().unwrap().plan,
        &pairs[1].as_ref().unwrap().plan
    ));
    assert!(plan_sequence(&core, &tracks, &|| false).is_none());
}

#[test]
fn changed_entrance_invalidates_the_suffix_and_wrap_is_a_new_pass() {
    let (_dir, core, tracks) = super::super::tests::fixture();
    *core.mix_preparation.playlist.lock().unwrap() = tracks.clone();
    let pairs = plan_sequence(&core, &tracks, &|| true).unwrap();
    let mut anchor = (*pairs[0].as_ref().unwrap().plan).clone();
    anchor.t_in_b = 5.;
    anchor.t_end_b = 5.5;
    {
        let mut sequence = core.mix_preparation.sequence.lock().unwrap();
        sequence.pairs = pairs.clone();
        sequence.anchor = Some(Arc::new(anchor));
    }
    let changed = plan_sequence(&core, &tracks, &|| true).unwrap();
    let next = &changed[1].as_ref().unwrap().plan;
    assert!(next.t_in_a >= 6.5);
    assert!(!Arc::ptr_eq(next, &pairs[1].as_ref().unwrap().plan));
    let mut wrap = (**next).clone();
    wrap.summary.as_mut().unwrap().pair = (tracks[2], tracks[0]);
    core.mix_preparation.sequence.lock().unwrap().anchor = Some(Arc::new(wrap));
    let wrapped = plan_sequence(&core, &tracks, &|| true).unwrap();
    assert!(wrapped[2].is_some() && wrapped[0].is_some());
    assert!(
        wrapped[1].is_none(),
        "do not overlay next lap's entry on the current outgoing track"
    );
}

#[test]
fn forecast_does_not_ignore_a_manual_out_consumed_by_the_entry() {
    let (_dir, core, tracks) = super::super::tests::fixture();
    *core.mix_preparation.playlist.lock().unwrap() = tracks.clone();
    let pairs = plan_sequence(&core, &tracks, &|| true).unwrap();
    let mut anchor = (*pairs[0].as_ref().unwrap().plan).clone();
    anchor.t_in_b = 1.;
    anchor.t_end_b = 5.;
    core.mix_preparation.sequence.lock().unwrap().anchor = Some(Arc::new(anchor));
    core.library
        .set_cue(
            tracks[1],
            7,
            4 * 48000,
            mixless_protocol::CueKind::Out,
            true,
        )
        .unwrap();
    let forecast = plan_sequence(&core, &tracks, &|| true).unwrap();
    assert!(forecast[0].is_some() && forecast[1].is_none());
}

#[test]
fn reordered_queue_continues_from_the_armed_incoming_track() {
    let (_dir, core, tracks) = super::super::tests::fixture();
    *core.mix_preparation.playlist.lock().unwrap() = tracks.clone();
    let pairs = plan_sequence(&core, &tracks, &|| true).unwrap();
    let anchor = pairs[0].as_ref().unwrap().plan.clone();
    core.mix_preparation.sequence.lock().unwrap().anchor = Some(anchor.clone());
    // The already armed 1 -> 2 remains audible while the order becomes 3,1,2.
    let reordered = vec![tracks[2], tracks[0], tracks[1]];
    *core.mix_preparation.playlist.lock().unwrap() = reordered.clone();
    let updated = plan_sequence(&core, &reordered, &|| true).unwrap();
    assert!(Arc::ptr_eq(&updated[1].as_ref().unwrap().plan, &anchor));
    assert_eq!(
        updated[2]
            .as_ref()
            .unwrap()
            .plan
            .summary
            .as_ref()
            .unwrap()
            .pair,
        (tracks[1], tracks[2])
    );
    assert!(updated[0].is_none());
    // A committed pair outside the new adjacency is still the cursor.
    let reordered = vec![tracks[0], tracks[2], tracks[1]];
    *core.mix_preparation.playlist.lock().unwrap() = reordered.clone();
    let updated = plan_sequence(&core, &reordered, &|| true).unwrap();
    // 2's new successor is the playing 1: no false second occurrence of 1.
    assert!(updated.iter().all(Option::is_none));
}

/// Opt-in audit uses the same desktop preparation and cache reuse as AUTO.
/// MIXLESS_SET_AUDIT_DB must be a SQLite backup, never the live library.
#[test]
#[ignore]
fn audit_real_set_sequence() {
    let db = std::env::var("MIXLESS_SET_AUDIT_DB").expect("SQLite backup path");
    let output = std::path::PathBuf::from(std::env::var("MIXLESS_SET_AUDIT_OUTPUT").unwrap());
    let playlist = std::env::var("MIXLESS_SET_AUDIT_PLAYLIST")
        .unwrap()
        .parse()
        .unwrap();
    let (_dir, mut core, _) = super::super::tests::fixture();
    let mutable = Arc::get_mut(&mut core).unwrap();
    mutable.library = mixless_library::Library::open(
        &std::fs::canonicalize(&db).expect("existing SQLite backup"),
    )
    .unwrap();
    if let Ok(cache) = std::env::var("MIXLESS_SET_AUDIT_STEMS") {
        mutable.stems = Some(mixless_stems::Processor::new(
            output.join("models"),
            cache.into(),
            false,
        ));
    }
    let tracks = core
        .library
        .playlist_tracks(mixless_protocol::PlaylistId(playlist))
        .unwrap();
    assert!(tracks.len() >= 2, "playlist missing or empty");
    let ids: Vec<_> = tracks.iter().map(|t| t.id).collect();
    *core.mix_preparation.playlist.lock().unwrap() = ids.clone();
    let pairs = plan_sequence(&core, &ids, &|| true).unwrap();
    assert!(
        pairs[..ids.len() - 1].iter().all(Option::is_some),
        "incomplete set; missing edges {:?}",
        pairs
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_none())
            .map(|(i, _)| (i, &tracks[i].title))
            .collect::<Vec<_>>()
    );
    let plans: Vec<_> = pairs.iter().flatten().map(|p| &*p.plan).collect();
    for (i, p) in plans.iter().enumerate() {
        if i > 0 {
            assert!(
                p.t_in_a
                    >= plans[i - 1].t_end_b
                        + STAGING_SECONDS * plans[i - 1].incoming_offset_end.rate
            );
            assert_eq!(p.outgoing_offset, plans[i - 1].incoming_offset_end);
        }
        println!(
            "{} -> {}: {:?}, A {:.3}..{:.3}, B {:.3}..{:.3}, offset {:?}",
            tracks[i].title,
            tracks[i + 1].title,
            p.summary.as_ref().unwrap().strategy,
            p.t_in_a,
            p.t_out_a,
            p.t_in_b,
            p.t_end_b,
            p.incoming_offset_end
        );
    }
    core.mix_preparation.sequence.lock().unwrap().pairs = pairs.clone();
    // Background refresh and runtime staging must consume these exact plans.
    for _ in 0..2 {
        let again = plan_sequence(&core, &ids, &|| true).unwrap();
        for (a, b) in pairs.iter().flatten().zip(again.iter().flatten()) {
            assert!(
                Arc::ptr_eq(&a.plan, &b.plan),
                "forecast changed without new input"
            );
        }
    }
    for i in 0..plans.len() {
        let a = crate::analysis::prepare(&core, ids[i]).unwrap();
        let b = crate::analysis::prepare(&core, ids[i + 1]).unwrap();
        let (entry, ready, offset) = if i == 0 {
            let entry = cue_frame(&core, &a).unwrap() as f32 / a.analysis.sample_rate as f32;
            (entry, entry, PerformanceOffset::identity())
        } else {
            (
                plans[i - 1].t_in_b,
                plans[i - 1].t_end_b,
                plans[i - 1].incoming_offset_end,
            )
        };
        let staged = super::super::pair::scheduled_pair(
            &core,
            &a,
            &b,
            ready + 0.75 * offset.rate,
            entry,
            offset,
            PerformanceOffset::identity(),
            Some(ids[(i + 2) % ids.len()]),
            stem_playback_ready(&core, &a) && stem_playback_ready(&core, &b),
        )
        .unwrap();
        assert!(
            Arc::ptr_eq(&staged.plan, &pairs[i].as_ref().unwrap().plan),
            "runtime replaced forecast for pair {i}"
        );
    }
    println!(
        "All {} runtime stages reused the published set plans",
        plans.len()
    );
    let analyses: Vec<_> = ids
        .iter()
        .map(|id| crate::analysis::prepare(&core, *id).unwrap().analysis)
        .collect();
    let moves: Vec<_> = analyses.iter().zip(&plans).enumerate().map(|(i, (a, p))| {
        let from = if i == 0 { 0. } else { plans[i-1].t_end_b };
        let moves = mixless_mixplan::performance_moves(a, from, p.t_in_a, core.stems.is_some(), mixless_protocol::LiveMoves::Subtle);
        serde_json::json!({"track": tracks[i].title, "moves": moves.iter().map(|m| serde_json::json!({"label":m.label,"start":m.start_sec,"end":m.end_sec,"nodes":m.curve.nodes})).collect::<Vec<_>>()})
    }).collect();
    std::fs::create_dir_all(&output).unwrap();
    for (name, value) in [
        ("plans.json", serde_json::to_value(&plans).unwrap()),
        (
            "analyses.json",
            serde_json::to_value(analyses.iter().map(|a| &**a).collect::<Vec<_>>()).unwrap(),
        ),
        (
            "paths.json",
            serde_json::to_value(tracks.iter().map(|t| &t.path).collect::<Vec<_>>()).unwrap(),
        ),
        ("moves.json", serde_json::to_value(moves).unwrap()),
    ] {
        std::fs::write(
            output.join(name),
            serde_json::to_vec_pretty(&value).unwrap(),
        )
        .unwrap();
    }
}
