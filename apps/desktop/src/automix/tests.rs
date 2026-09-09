use super::*;
use std::sync::{Mutex, mpsc::channel};
use std::time::Instant;

fn fixture() -> (tempfile::TempDir, Arc<AppCore>, Vec<TrackId>) {
    let dir = tempfile::tempdir().unwrap();
    let library = mixless_library::Library::open(&dir.path().join("library.db")).unwrap();
    let engine = mixless_engine::Engine::new(mixless_engine::EngineConfig {
        offline: true, sample_rate: 48_000, block_frames: 256,
    }).unwrap();
    let core = Arc::new(AppCore {
        analysis: Default::default(), mix_preparation: Default::default(), automix_commit: Mutex::new(()),
        settings: crate::settings::SettingsStore::load(dir.path().join("preferences.json")),
        midi_error: Mutex::new(None), settings_apply: Mutex::new(()), engine, library,
        analyzer: mixless_analyze::Analyzer::with_options(mixless_analyze::AnalysisOptions {
            native_vocals: false, ..Default::default()
        }),
        acquired_dir: dir.path().join("acquired"), midi: Mutex::new(None), deck_load: Mutex::new(()),
    });
    let mut tracks = vec![];
    for i in 0..3 {
        let path = dir.path().join(format!("track-{i}.wav"));
        let frames = 48_000 * 8;
        let size = (frames * 4) as u32;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF"); bytes.extend_from_slice(&(size + 36).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt "); bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes()); bytes.extend_from_slice(&192_000u32.to_le_bytes());
        bytes.extend_from_slice(&4u16.to_le_bytes()); bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data"); bytes.extend_from_slice(&size.to_le_bytes());
        for frame in 0..frames {
            let v = ((std::f32::consts::TAU * (110. + i as f32 * 33.) * frame as f32 / 48_000.).sin() * 5000.) as i16;
            bytes.extend_from_slice(&v.to_le_bytes()); bytes.extend_from_slice(&v.to_le_bytes());
        }
        std::fs::write(&path,bytes).unwrap();
        let id = core.library.register_local_files(&[path]).unwrap()[0];
        crate::analysis::prepare(&core,id).unwrap();
        tracks.push(id);
    }
    (dir,core,tracks)
}

#[test]
fn auto_repeats_playlist_and_can_be_disabled_and_reenabled_during_overlap() {
    let (_dir,core,tracks) = fixture();
    let epoch = Arc::new(AtomicU64::new(1));
    let shuffle = Arc::new(AtomicBool::new(false));
    let spawn = |generation| {
        let (tx,rx) = channel();
        let (core,tracks,epoch,shuffle) = (core.clone(),tracks.clone(),epoch.clone(),shuffle.clone());
        let worker = std::thread::spawn(move || run(core,tracks,shuffle,epoch,generation,tx));
        (worker,rx)
    };
    let (worker,rx) = spawn(1);
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
            if last_pair != Some(pair) { history.push(pair); last_pair = Some(pair); }
            if history.len() >= 5 && snapshot.decks.iter().all(|d| d.playing) && snapshot.automix_progress > 0.1 {
                let _commit = core.automix_commit.lock().unwrap();
                epoch.store(2,Ordering::Release);
                core.engine.dispatch(Command::StopAutomix).unwrap();
                let stopped = core.engine.snapshot();
                assert!(stopped.decks.iter().all(|d| d.playing));
                assert_eq!(snapshot.xfader,stopped.xfader);
                disabled = true;
                break;
            }
        }
        if let Ok(AutomixMsg::Done(result)) = rx.try_recv() { panic!("AUTO ended early: {result:?}, pairs={history:?}"); }
        std::thread::sleep(Duration::from_millis(3));
    }
    epoch.store(2,Ordering::Release);
    worker.join().unwrap();
    assert!(disabled,"did not reach second playlist cycle: {history:?}");
    assert_eq!(history[0],(tracks[0],tracks[1]));
    assert_eq!(history[1],(tracks[1],tracks[2]));
    assert_eq!(history[2],(tracks[2],tracks[0]));
    epoch.store(3,Ordering::Release);
    let (worker,rx) = spawn(3);
    let deadline = Instant::now() + Duration::from_secs(4);
    let mut resumed = false;
    while Instant::now() < deadline {
        core.engine.render_offline(480);
        if core.engine.snapshot().automix_on { resumed = true; break; }
        if let Ok(AutomixMsg::Done(result)) = rx.try_recv() { panic!("AUTO could not reenable: {result:?}"); }
        std::thread::sleep(Duration::from_millis(3));
    }
    epoch.store(4,Ordering::Release);
    worker.join().unwrap();
    assert!(resumed);
}

#[test]
fn cancelled_load_keeps_the_deck_and_cached_load_reuses_waveform() {
    let (_dir,core,tracks) = fixture();
    crate::analysis::load(&core,DeckId::A,tracks[0],||true).unwrap();
    let wave = core.engine.deck_waveform(DeckId::A).unwrap();
    assert!(!crate::analysis::load(&core,DeckId::A,tracks[1],||false).unwrap());
    assert_eq!(core.engine.snapshot().decks[0].track_id,Some(tracks[0]));
    crate::analysis::load(&core,DeckId::B,tracks[0],||true).unwrap();
    assert!(Arc::ptr_eq(&wave,&core.engine.deck_waveform(DeckId::B).unwrap()));
}
