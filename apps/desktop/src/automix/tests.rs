use super::*;
use std::sync::{Mutex, mpsc::channel};
use std::time::Instant;

pub(crate) fn fixture() -> (tempfile::TempDir, Arc<AppCore>, Vec<TrackId>) {
    let dir = tempfile::tempdir().unwrap();
    let library = mixless_library::Library::open(&dir.path().join("library.db")).unwrap();
    let engine = mixless_engine::Engine::new(mixless_engine::EngineConfig {
        offline: true,
        sample_rate: 48_000,
        block_frames: 256,
    })
    .unwrap();
    let core = Arc::new(AppCore {
        analysis: Default::default(),
        mix_preparation: Default::default(),
        automix_commit: Mutex::new(()),
        settings: crate::settings::SettingsStore::load(dir.path().join("preferences.json")),
        midi_error: Mutex::new(None),
        settings_apply: Mutex::new(()),
        engine,
        library,
        analyzer: mixless_analyze::Analyzer::with_options(mixless_analyze::AnalysisOptions {
            native_vocals: false,
            ..Default::default()
        }),
        acquired_dir: dir.path().join("acquired"),
        midi: Mutex::new(None),
        deck_load: Mutex::new(()),
    });
    let mut tracks = vec![];
    for i in 0..3 {
        let path = dir.path().join(format!("track-{i}.wav"));
        let frames = 48_000 * 8;
        let size = (frames * 4) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(size + 36).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&192_000u32.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&size.to_le_bytes());
        for frame in 0..frames {
            let v = ((std::f32::consts::TAU * (110. + i as f32 * 33.) * frame as f32 / 48_000.)
                .sin()
                * 5000.) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&path, bytes).unwrap();
        let id = core.library.register_local_files(&[path]).unwrap()[0];
        crate::analysis::prepare(&core, id).unwrap();
        tracks.push(id);
    }
    (dir, core, tracks)
}

#[test]
fn auto_repeats_playlist_and_can_resume_from_a_manual_overlap() {
    let (_dir, core, tracks) = fixture();
    let epoch = Arc::new(AtomicU64::new(1));
    let shuffle = Arc::new(AtomicBool::new(false));
    let spawn = |generation| {
        let (tx, rx) = channel();
        let (core, tracks, epoch, shuffle) =
            (core.clone(), tracks.clone(), epoch.clone(), shuffle.clone());
        let worker = std::thread::spawn(move || run(core, tracks, shuffle, epoch, generation, tx));
        (worker, rx)
    };
    let (worker, rx) = spawn(1);
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut history = vec![];
    let mut last_pair = None;
    let mut disabled = false;
    while Instant::now() < deadline {
        let audio = core.engine.render_offline(480);
        assert!(audio.iter().all(|s| s.is_finite() && s.abs() <= 1.));
        let snapshot = core.engine.snapshot();
        if snapshot.automix_on {
            let pair = core.engine.automix_summary().unwrap().pair;
            if last_pair != Some(pair) {
                history.push(pair);
                last_pair = Some(pair);
            }
            if history.len() >= 5 && snapshot.automix_progress > 0.1 {
                let _commit = core.automix_commit.lock().unwrap();
                epoch.store(2, Ordering::Release);
                core.engine.dispatch(Command::StopAutomix).unwrap();
                let stopped = core.engine.snapshot();
                assert_eq!(
                    stopped.decks.each_ref().map(|d| d.playing),
                    snapshot.decks.each_ref().map(|d| d.playing)
                );
                assert_eq!(snapshot.xfader, stopped.xfader);
                disabled = true;
                break;
            }
        }
        for msg in rx.try_iter() {
            match msg {
                AutomixMsg::Done(result) => {
                    panic!("AUTO ended early: {result:?}, pairs={history:?}")
                }

                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    epoch.store(2, Ordering::Release);
    worker.join().unwrap();
    assert!(disabled, "did not reach second playlist cycle: {history:?}");
    assert_eq!(history[0], (tracks[0], tracks[1]));
    assert_eq!(history[1], (tracks[1], tracks[2]));
    assert_eq!(history[2], (tracks[2], tracks[0]));
    // Reenable while the user is manually playing both decks. AUTO settles
    // the audible overlap, then uses the freed deck without restarting the song.
    for deck in [DeckId::A, DeckId::B] {
        let d = core.engine.snapshot().deck(deck).clone();
        if !d.playing {
            core.engine
                .cue_loaded_track(deck, d.track_id.unwrap(), 0)
                .unwrap();
            core.engine
                .dispatch(Command::SetChannelFader { deck, value: 0.8 })
                .unwrap();
            core.engine.dispatch(Command::PlayPause { deck }).unwrap();
        }
    }
    core.engine
        .dispatch(Command::SetCrossfader { value: 0. })
        .unwrap();
    epoch.store(3, Ordering::Release);
    let (worker, rx) = spawn(3);
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut resumed = false;
    while Instant::now() < deadline {
        core.engine.render_offline(480);
        if core.engine.snapshot().automix_on {
            resumed = true;
            break;
        }
        if let Ok(AutomixMsg::Done(result)) = rx.try_recv() {
            panic!("AUTO could not reenable: {result:?}");
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    epoch.store(4, Ordering::Release);
    worker.join().unwrap();
    assert!(resumed);
}

#[test]
fn cancelled_load_keeps_the_deck_and_cached_load_reuses_waveform() {
    let (_dir, core, tracks) = fixture();
    crate::analysis::load(&core, DeckId::A, tracks[0], || true).unwrap();
    let wave = core.engine.deck_waveform(DeckId::A).unwrap();
    assert!(!crate::analysis::load(&core, DeckId::A, tracks[1], || false).unwrap());
    assert_eq!(core.engine.snapshot().decks[0].track_id, Some(tracks[0]));
    crate::analysis::load(&core, DeckId::B, tracks[0], || true).unwrap();
    assert!(Arc::ptr_eq(
        &wave,
        &core.engine.deck_waveform(DeckId::B).unwrap()
    ));
}

#[test]
fn track_in_and_out_markers_apply_to_their_own_transition_side() {
    use mixless_protocol::CueKind;
    let (_dir, core, tracks) = fixture();
    let a = crate::analysis::prepare(&core, tracks[0]).unwrap();
    let b = crate::analysis::prepare(&core, tracks[1]).unwrap();
    for id in [tracks[0], tracks[1]] {
        core.library.set_cue(id, 0, 0, CueKind::In, true).unwrap();
        core.library
            .set_cue(id, 7, 7 * 48000, CueKind::Out, true)
            .unwrap();
    }
    let p = super::preparation::pair(&core, &a, &b, 0., Default::default(), Default::default())
        .unwrap();
    assert!(p.t_in_a <= 7. && p.t_out_a >= 7.);
    assert_eq!(p.t_in_b, 0.);
    assert!(
        p.t_end_b < 7.,
        "incoming OUT must not force its entire song into the entry mix"
    );
    let late = super::preparation::pair(&core, &a, &b, 7.1, Default::default(), Default::default())
        .unwrap();
    assert!(late.t_in_a >= 7.1 && late.t_out_a <= a.analysis.duration_sec);
    assert_eq!(late.t_in_b, 0.);
}

#[test]
fn empty_decks_start_first_track_in_shuffle_with_a_cancellable_audio_clock_fade() {
    let (_dir, core, tracks) = fixture();
    core.engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    assert!(!core.engine.snapshot().decks[0].playing);
    let epoch = Arc::new(AtomicU64::new(1));
    let (tx, rx) = channel();
    let worker = {
        let core = core.clone();
        let tracks = tracks.clone();
        let epoch = epoch.clone();
        std::thread::spawn(move || run(core, tracks, Arc::new(AtomicBool::new(true)), epoch, 1, tx))
    };
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut levels = vec![];
    let mut first = None;
    while Instant::now() < deadline {
        let s = core.engine.snapshot();
        if s.decks[0].playing {
            first.get_or_insert(s.decks[0].track_id);
            levels.push(s.decks[0].fader);
            if s.decks[0].fader > 0.35 {
                break;
            }
        }
        core.engine.render_offline(480);
        for msg in rx.try_iter() {
            if let AutomixMsg::Done(r) = msg {
                panic!("cold start failed: {r:?}");
            }
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    assert_eq!(first, Some(Some(tracks[0])));
    assert!(
        levels.iter().any(|v| *v < 0.05) && levels.iter().any(|v| *v > 0.3),
        "{levels:?}"
    );
    assert!(levels.windows(2).all(|v| v[1] >= v[0]));
    let before = {
        let _commit = core.automix_commit.lock().unwrap();
        epoch.store(2, Ordering::Release);
        core.engine.dispatch(Command::StopAutomix).unwrap();
        core.engine.snapshot()
    };
    worker.join().unwrap();
    core.engine.render_offline(4096);
    let after = core.engine.snapshot();
    assert_eq!(after.decks[0].fader, before.decks[0].fader);
    assert_eq!(after.decks[0].filter_amount, before.decks[0].filter_amount);
    assert!(after.decks[0].playing);
}

#[test]
fn cold_start_skips_a_missing_first_file_and_becomes_audible_without_manual_play() {
    let (_dir, core, tracks) = fixture();
    std::fs::remove_file(core.library.get_track(tracks[0]).unwrap().path).unwrap();
    let epoch = Arc::new(AtomicU64::new(1));
    let (tx, rx) = channel();
    let worker = {
        let core = core.clone();
        let tracks = tracks.clone();
        let epoch = epoch.clone();
        std::thread::spawn(move || {
            run(core, tracks, Arc::new(AtomicBool::new(false)), epoch, 1, tx)
        })
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut audible = false;
    while Instant::now() < deadline {
        let audio = core.engine.render_offline(480);
        let snap = core.engine.snapshot();
        if snap.decks[0].track_id == Some(tracks[1])
            && snap.decks[0].playing
            && audio.iter().any(|v| v.abs() > 0.01)
        {
            audible = true;
            break;
        }
        for msg in rx.try_iter() {
            if let AutomixMsg::Done(result) = msg {
                panic!("cold start ended: {result:?}");
            }
        }
        std::thread::sleep(Duration::from_millis(3));
    }
    epoch.store(2, Ordering::Release);
    worker.join().unwrap();
    assert!(audible, "AUTO must load and play the next usable file");
}

#[test]
fn terminal_drop_handoff_launches_at_eof_on_the_render_clock() {
    let (_dir, core, tracks) = fixture();
    for (deck, id) in [(DeckId::A, tracks[0]), (DeckId::B, tracks[1])] {
        crate::analysis::load(&core, deck, id, || true).unwrap();
    }
    let mut a = (*crate::analysis::prepare(&core, tracks[0]).unwrap().analysis).clone();
    let b = crate::analysis::prepare(&core, tracks[1]).unwrap();
    a.sections = vec![
        mixless_protocol::Section {
            start_sec: 0.,
            end_sec: 2.,
            label: mixless_protocol::SectionLabel::Intro,
        },
        mixless_protocol::Section {
            start_sec: 2.,
            end_sec: 8.,
            label: mixless_protocol::SectionLabel::Drop,
        },
    ];
    let plan = mixless_mixplan::short_handoff(
        &mixless_mixplan::PlanContext {
            outgoing: &a,
            incoming: &b.analysis,
            cues_out: &[],
            cues_in: &[],
            offset_a: Default::default(),
            offset_b: Default::default(),
        },
        0.,
    );
    core.engine
        .dispatch(Command::SetCrossfader { value: -1. })
        .unwrap();
    core.engine
        .dispatch(Command::PlayPause { deck: DeckId::A })
        .unwrap();
    core.engine.load_plan(plan).unwrap();
    for _ in 0..799 {
        core.engine.render_offline(480);
        assert!(!core.engine.snapshot().decks[1].playing);
    }
    for _ in 0..100 {
        core.engine.render_offline(480);
    }
    let snapshot = core.engine.snapshot();
    assert!(snapshot.decks[1].playing && snapshot.decks[1].frame > 0);
    assert!(!snapshot.decks[0].playing);
    assert!(!snapshot.automix_on && snapshot.automix_progress >= 1.);
}
