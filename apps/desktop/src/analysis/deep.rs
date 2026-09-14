use super::*;
use std::collections::HashSet;
use std::sync::mpsc::{Receiver, RecvTimeoutError};

/// FIFO order lives in `queue`; `pending` additionally covers the job the
/// worker is running. `sender` only wakes the worker.
#[derive(Default)]
pub(super) struct Jobs {
    pub pending: Mutex<HashSet<TrackId>>,
    completed: Mutex<HashMap<TrackId, u64>>,
    queue: Mutex<VecDeque<TrackId>>,
    sender: OnceLock<Sender<()>>,
}

impl Jobs {
    /// Queue `id` at the back, or move it to the front when `priority` is set.
    /// Returns false only when the worker is already running this track.
    fn enqueue(&self, id: TrackId, priority: bool) -> bool {
        let mut queue = self.queue.lock().expect("deep queue");
        if let Some(pos) = queue.iter().position(|queued| *queued == id) {
            if priority && pos > 0 {
                queue.remove(pos);
                queue.push_front(id);
            }
            return true;
        }
        if !self.pending.lock().expect("deep jobs").insert(id) {
            return false;
        }
        if priority {
            queue.push_front(id);
        } else {
            queue.push_back(id);
        }
        true
    }

    fn notify(&self, core: &Arc<AppCore>) {
        let _ = self
            .sender
            .get_or_init(|| {
                let (tx, rx) = channel::<()>();
                let weak = Arc::downgrade(core);
                std::thread::spawn(move || worker(weak, rx));
                tx
            })
            .send(());
    }

    #[cfg(test)]
    fn queued(&self) -> Vec<TrackId> {
        self.queue
            .lock()
            .expect("deep queue")
            .iter()
            .copied()
            .collect()
    }
}

fn worker(weak: std::sync::Weak<AppCore>, rx: Receiver<()>) {
    // Releasing the 165 MB model set on every idle gap makes the next queued
    // track pay a full reload. Only drop it after a sustained quiet spell.
    const IDLE_RELEASE: std::time::Duration = std::time::Duration::from_secs(30);
    loop {
        match rx.recv_timeout(IDLE_RELEASE) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => {
                let Some(core) = weak.upgrade() else { break };
                if core
                    .analysis
                    .deep_jobs
                    .queue
                    .lock()
                    .expect("deep queue")
                    .is_empty()
                {
                    if let Some(processor) = &core.stems {
                        processor.release_models();
                    }
                }
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
        let Some(core) = weak.upgrade() else { break };
        loop {
            let id = {
                let mut queue = core.analysis.deep_jobs.queue.lock().expect("deep queue");
                // A send always follows its push under this lock, so stale
                // wake-ups drained here can never hide a queued track.
                for _ in rx.try_iter() {}
                queue.pop_front()
            };
            let Some(id) = id else { break };
            job(&core, id);
        }
    }
}

fn job(core: &Arc<AppCore>, id: TrackId) {
    let revision = core.analysis.content_revision.load(Ordering::Acquire);
    let generation = core.analysis.epoch(id).load(Ordering::Acquire);
    let result = run(core, id);
    if let Ok(prepared) = &result {
        for deck in [DeckId::A, DeckId::B] {
            stems::attach_stems(core, deck, id, &prepared.track.content_hash);
        }
    }
    if let Err(error) = &result {
        tracing::warn!(?id,%error,"Stem analysis did not publish");
    }
    core.analysis
        .deep_jobs
        .pending
        .lock()
        .expect("deep jobs")
        .remove(&id);
    if core.analysis.epoch(id).load(Ordering::Acquire) == generation {
        core.analysis
            .deep_jobs
            .completed
            .lock()
            .expect("deep completion")
            .insert(id, generation);
        match result {
            Ok(prepared) => core.analysis.set(id, ready_status(&prepared)),
            Err(error) => {
                let status = match core.library.get_track(id) {
                    Ok(track) if track.analyzed => Status::Basic(track.content_hash, error),
                    _ => Status::Failed(error),
                };
                core.analysis.set(id, status);
            }
        }
    } else {
        schedule(core, id);
    }
    if core.analysis.content_revision.load(Ordering::Acquire) != revision {
        crate::automix::refresh_previews(core);
    }
}

fn queueable(core: &AppCore, id: TrackId) -> bool {
    core.stems.is_some()
        && core.settings.get().deep_analysis
        && core
            .analysis
            .deep_jobs
            .completed
            .lock()
            .expect("deep completion")
            .get(&id)
            != Some(&core.analysis.epoch(id).load(Ordering::Acquire))
}

pub fn schedule(core: &Arc<AppCore>, id: TrackId) {
    if !queueable(core, id) {
        return;
    }
    if core.analysis.deep_jobs.enqueue(id, false) {
        core.analysis.deep_jobs.notify(core);
    }
}

/// A track on a deck is heard soon; it jumps ahead of library preparation.
/// A job already running is left to finish.
pub fn promote(core: &Arc<AppCore>, id: TrackId) {
    if !queueable(core, id) {
        return;
    }
    if core.analysis.deep_jobs.enqueue(id, true) {
        core.analysis.deep_jobs.notify(core);
    }
}

pub(super) fn retry_for_playback(core: &Arc<AppCore>, id: TrackId) {
    if !core.settings.get().deep_analysis {
        return;
    }
    core.analysis
        .deep_jobs
        .completed
        .lock()
        .expect("deep completion")
        .remove(&id);
    promote(core, id);
}

pub(super) fn ready_status(prepared: &PreparedTrack) -> Status {
    match &prepared.warning {
        Some(warning) => Status::Basic(prepared.track.content_hash.clone(), warning.clone()),
        None => Status::Ready(prepared.track.content_hash.clone()),
    }
}

fn run(core: &AppCore, id: TrackId) -> Result<PreparedTrack, String> {
    let prepared = prepare(core, id)?;
    let Some(processor) = core
        .stems
        .as_ref()
        .filter(|_| core.settings.get().deep_analysis)
    else {
        return Ok(prepared);
    };
    if prepared
        .analysis
        .stems
        .as_ref()
        .is_some_and(|s| mixless_stems::is_current(s) && s.valid(prepared.analysis.duration_sec))
        && processor
            .cached(&prepared.track.content_hash, prepared.analysis.duration_sec)
            .is_ok_and(|s| s.is_some())
    {
        core.analysis.set(id, ready_status(&prepared));
        return Ok(prepared);
    }
    let track = &prepared.track;
    let epoch = core.analysis.epoch(id);
    let generation = epoch.load(Ordering::Acquire);
    core.analysis.set(
        track.id,
        Status::Enhancing("Waiting for stem analysis".into()),
    );
    let active = || {
        !core.shutting_down.load(Ordering::Acquire)
            && core.settings.get().deep_analysis
            && epoch.load(Ordering::Acquire) == generation
    };
    let result = processor.analyze(
        &track.content_hash,
        prepared.analysis.duration_sec,
        || playback::pcm(core, track).map_err(mixless_stems::Error::Model),
        &mut |progress| {
            use mixless_stems::Progress::*;
            let label = match progress {
                Downloading { percent, .. } => format!("Downloading analysis model · {percent}%"),
                Loading => "Loading analysis models".into(),
                Separating(p) => format!("Separating stems · {p}%"),
                Notes { stem, percent } => format!("Analyzing {stem} notes · {percent}%"),
                Saving => "Saving stem analysis".into(),
            };
            if active() {
                core.analysis.set(track.id, Status::Enhancing(label));
            }
        },
        &active,
    );
    let lock = core
        .analysis
        .locks
        .lock()
        .expect("analysis locks")
        .entry(id)
        .or_default()
        .clone();
    let _publish = lock
        .lock()
        .map_err(|_| "Analysis publication lock poisoned")?;
    if !active() {
        return Ok(prepared);
    }
    let evidence = match result {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(track=?track.id,error=%e,"Deep analysis unavailable; retaining basic analysis");
            let mut fallback = prepared;
            fallback.warning = Some(e.to_string());
            core.analysis.set(id, ready_status(&fallback));
            replace_cached(core, &fallback);
            return Ok(fallback);
        }
    };
    if core
        .library
        .verified_content_hash(Path::new(&track.path))
        .map_err(|e| e.to_string())?
        != track.content_hash
    {
        return Err("File changed during stem analysis; retry".into());
    }
    let mut analysis = (*prepared.analysis).clone();
    if analysis
        .stems
        .as_ref()
        .is_some_and(mixless_stems::is_current)
    {
        // Regenerating evicted PCM must not blend the same bar evidence twice.
        analysis.stems = Some(evidence);
    } else if !mixless_analyze::Analyzer::apply_stems(&mut analysis, evidence) {
        return Err("Stem analysis did not cover the complete track".into());
    }
    core.library
        .finish_analysis(
            &analysis,
            &track.content_hash,
            mixless_analyze::ANALYSIS_VERSION,
        )
        .map_err(|e| e.to_string())?;
    core.library
        .replace_auto_cues(id, &mixless_analyze::automatic_cues(&analysis))
        .map_err(|e| e.to_string())?;
    let enhanced = PreparedTrack {
        track: core.library.get_track(id).map_err(|e| e.to_string())?,
        analysis: Arc::new(analysis),
        wave: prepared.wave,
        warning: None,
    };
    replace_cached(core, &enhanced);
    reanalysis::update_loaded(core, &enhanced, false)?;
    core.analysis
        .updates
        .lock()
        .expect("analysis updates")
        .insert(id, enhanced.track.clone());
    core.analysis
        .latest
        .lock()
        .expect("analysis metadata")
        .insert(id, enhanced.track.clone());
    core.analysis.set(id, ready_status(&enhanced));
    core.analysis
        .content_revision
        .fetch_add(1, Ordering::AcqRel);
    Ok(enhanced)
}

fn replace_cached(core: &AppCore, prepared: &PreparedTrack) {
    let mut cache = core.analysis.prepared.lock().expect("prepared cache");
    cache.retain(|p| p.track.id != prepared.track.id);
    cache.push_back(prepared.clone());
    while cache.len() > 8 {
        cache.pop_front();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_models_keep_manual_audio_and_basic_analysis_usable() {
        let (dir, mut core, tracks) = crate::automix::tests::fixture();
        Arc::get_mut(&mut core).unwrap().stems = Some(mixless_stems::Processor::new(
            dir.path().join("missing-models"),
            dir.path().join("stems"),
            false,
        ));
        let id = tracks[0];
        core.analysis.forget(id);
        let prepared = run(&core, id).unwrap();
        assert!(prepared.analysis.stems.is_none());
        assert!(prepared.warning.is_some());
        assert!(matches!(
            core.analysis.status(&prepared.track),
            Status::Basic(_, _)
        ));
        load_manual(&core, DeckId::A, id, || true).unwrap();
        assert_eq!(core.engine.snapshot().decks[0].track_id, Some(id));
    }
    #[test]
    #[ignore = "Requires pinned local ONNX models and MIXLESS_STEM_TEST_AUDIO; actual native inference"]
    fn native_models_reach_default_preparation_sql_cache_and_planner() {
        let (dir, mut core, _) = crate::automix::tests::fixture();
        let models = std::env::var_os("MIXLESS_MODEL_DIR").expect("MIXLESS_MODEL_DIR");
        let audio = std::env::var_os("MIXLESS_STEM_TEST_AUDIO").expect("MIXLESS_STEM_TEST_AUDIO");
        Arc::get_mut(&mut core).unwrap().stems = Some(mixless_stems::Processor::new(
            models.into(),
            dir.path().join("stems"),
            false,
        ));
        let id = core.library.import_file(Path::new(&audio)).unwrap();
        // Render the manual deck while the same track is being separated. This
        // exercises contention with real inference, not a sleeping mock worker.
        load_manual(&core, DeckId::A, id, || true).unwrap();
        core.engine
            .dispatch(mixless_protocol::Command::SetChannelFader {
                deck: DeckId::A,
                value: 1.,
            })
            .unwrap();
        core.engine
            .dispatch(mixless_protocol::Command::SetCrossfader { value: -1. })
            .unwrap();
        core.engine
            .dispatch(mixless_protocol::Command::PlayPause { deck: DeckId::A })
            .unwrap();
        prepare(&core, id).unwrap();
        schedule(&core, id);
        let mut timings = Vec::new();
        let began = std::time::Instant::now();
        while core
            .analysis
            .deep_jobs
            .pending
            .lock()
            .unwrap()
            .contains(&id)
        {
            let start = std::time::Instant::now();
            let output = core.engine.render_offline(128);
            timings.push(start.elapsed().as_secs_f64() * 1000.);
            assert!(output.iter().all(|v| v.is_finite()));
            assert!(began.elapsed().as_secs() < 600, "Inference stalled");
            if timings.len() == 100 {
                let fast = std::time::Instant::now();
                assert!(prepare(&core, id).is_ok());
                assert!(
                    fast.elapsed().as_millis() < 200,
                    "AutoMix preparation waited for the model"
                );
            }
            std::thread::sleep(std::time::Duration::from_micros(2667));
        }
        let prepared = prepare(&core, id).unwrap();
        timings.sort_by(f64::total_cmp);
        let p99 = timings[timings.len() * 99 / 100];
        eprintln!(
            "Native model concurrency: {} blocks, p99 {p99:.3} ms / 2.667 ms",
            timings.len()
        );
        assert!(p99 < 2.667);
        assert!(prepared.warning.is_none(), "{:?}", prepared.warning);
        assert!(
            core.engine.stems_ready(DeckId::A),
            "Default model queue must attach playable stems"
        );
        let stems = prepared
            .analysis
            .stems
            .as_ref()
            .expect("native evidence published");
        assert!(!stems.notes.is_empty());
        assert!(stems.valid(prepared.analysis.duration_sec));
        let saved = core
            .library
            .load_analysis(id, mixless_analyze::ANALYSIS_VERSION)
            .unwrap()
            .unwrap();
        assert_eq!(saved.stems.as_ref().unwrap().notes.len(), stems.notes.len());
        for cue in core.library.cues(id).unwrap() {
            assert_eq!(
                core.engine.snapshot().decks[0].cues[cue.index as usize],
                Some(cue.frame)
            );
        }
        core.analysis.forget(id);
        let start = std::time::Instant::now();
        let again = prepare(&core, id).unwrap();
        assert!(again.analysis.stems.is_some());
        assert!(
            start.elapsed().as_secs_f32() < 2.,
            "SQL cache should not repeat inference"
        );
        let plan = mixless_mixplan::Planner::new().plan_pair(
            &prepared.analysis,
            &again.analysis,
            &[],
            &[],
            Default::default(),
            Default::default(),
        );
        assert!(
            !plan
                .failure_reason
                .as_deref()
                .is_some_and(|e| e.contains("Invalid")),
            "{:?}",
            plan.failure_reason
        );
        super::stems::verify_native_stem_playback_under_inference(&core, id);
    }

    #[test]
    fn manual_load_is_audible_without_stem_analysis() {
        use mixless_protocol::Command;
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        load_manual(&core, DeckId::A, id, || true).unwrap();
        core.engine
            .dispatch(Command::SetChannelFader {
                deck: DeckId::A,
                value: 1.,
            })
            .unwrap();
        core.engine
            .dispatch(Command::SetCrossfader { value: -1. })
            .unwrap();
        core.engine
            .dispatch(Command::PlayPause { deck: DeckId::A })
            .unwrap();
        let mut peak = 0f32;
        for _ in 0..375 {
            let output = core.engine.render_offline(128);
            for sample in &output {
                peak = peak.max(sample.abs());
            }
        }
        let snap = core.engine.snapshot();
        eprintln!(
            "no-stems sanity: peak={peak:.5} playing={} frame={} stems_ready={}",
            snap.decks[0].playing, snap.decks[0].frame, snap.decks[0].stems_ready
        );
        assert!(peak > 0.01, "peak {peak}");
    }

    #[test]
    fn promote_moves_queued_tracks_to_the_front() {
        let jobs = Jobs::default();
        for i in [1, 2, 3] {
            assert!(jobs.enqueue(TrackId(i), false));
        }
        assert_eq!(jobs.queued(), [TrackId(1), TrackId(2), TrackId(3)]);
        assert!(jobs.enqueue(TrackId(3), true));
        assert_eq!(jobs.queued(), [TrackId(3), TrackId(1), TrackId(2)]);
        // A duplicate schedule does not reorder or duplicate the entry.
        assert!(jobs.enqueue(TrackId(1), false));
        assert_eq!(jobs.queued(), [TrackId(3), TrackId(1), TrackId(2)]);
        // The job the worker popped is still pending but no longer queued;
        // promoting it must not requeue it.
        let running = jobs.queue.lock().unwrap().pop_front().unwrap();
        assert_eq!(running, TrackId(3));
        assert!(!jobs.enqueue(TrackId(3), true));
        assert_eq!(jobs.queued(), [TrackId(1), TrackId(2)]);
    }
}
