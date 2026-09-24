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
    fn enqueue(&self, id: TrackId, priority: bool, queued: impl FnOnce()) -> bool {
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
        queued();
        true
    }

    fn notify(&self, core: &Arc<AppCore>) {
        let _ = self
            .sender
            .get_or_init(|| {
                let (tx, rx) = channel::<()>();
                let rx = Arc::new(Mutex::new(rx));
                let workers = core
                    .stems
                    .as_ref()
                    .map_or(1, |processor| processor.parallelism());
                for _ in 0..workers {
                    let weak = Arc::downgrade(core);
                    let rx = rx.clone();
                    std::thread::spawn(move || worker(weak, rx));
                }
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

fn worker(weak: std::sync::Weak<AppCore>, rx: Arc<Mutex<Receiver<()>>>) {
    // Releasing the 165 MB model set on every idle gap makes the next queued
    // track pay a full reload. Only drop it after a sustained quiet spell.
    const IDLE_RELEASE: std::time::Duration = std::time::Duration::from_secs(30);
    loop {
        // Release the receiver lock before processing; independent workers
        // must be able to receive the next queued recording during inference.
        let message = rx.lock().expect("deep wakeups").recv_timeout(IDLE_RELEASE);
        match message {
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
        let id = core
            .analysis
            .deep_jobs
            .queue
            .lock()
            .expect("deep queue")
            .pop_front();
        if let Some(id) = id {
            job(&core, id);
        }
    }
}

fn job(core: &Arc<AppCore>, id: TrackId) {
    let revision = core.analysis.content_revision.load(Ordering::Acquire);
    let generation = core.analysis.epoch(id).load(Ordering::Acquire);
    let result = run(core, id, generation);
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
            Ok(prepared) if prepared.analysis.stems.is_some() => {
                core.analysis.set_stems(id, StemStatus::Ready)
            }
            Ok(_) => {
                core.analysis
                    .stem_states
                    .lock()
                    .expect("stem states")
                    .remove(&id);
                core.analysis.revision.fetch_add(1, Ordering::Release);
            }
            Err(error) => core.analysis.set_stems(id, StemStatus::Failed(error)),
        }
    } else {
        schedule(core, id);
    }
    if core.analysis.content_revision.load(Ordering::Acquire) != revision {
        crate::automix::refresh_previews(core);
    }
}

fn queueable(core: &AppCore, id: TrackId) -> bool {
    mixless_stems::inference_supported()
        && core.stems.is_some()
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
    if core.analysis.deep_jobs.enqueue(id, false, || {
        core.analysis.set_stems(id, StemStatus::Queued)
    }) {
        core.analysis.deep_jobs.notify(core);
    }
}

/// A track on a deck is heard soon; it jumps ahead of library preparation.
/// A job already running is left to finish.
pub fn promote(core: &Arc<AppCore>, id: TrackId) {
    if !queueable(core, id) {
        return;
    }
    if core
        .analysis
        .deep_jobs
        .enqueue(id, true, || core.analysis.set_stems(id, StemStatus::Queued))
    {
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

fn run(core: &AppCore, id: TrackId, generation: u64) -> Result<PreparedTrack, String> {
    let epoch = core.analysis.epoch(id);
    let active = || {
        !core.shutting_down.load(Ordering::Acquire)
            && core.settings.get().deep_analysis
            && epoch.load(Ordering::Acquire) == generation
    };
    if !active() {
        return Err("Stem separation cancelled".into());
    }
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
        core.analysis.set_stems(id, StemStatus::Ready);
        return Ok(prepared);
    }
    let track = &prepared.track;
    if !active() {
        return Err("Stem separation cancelled".into());
    }
    core.analysis
        .set_stems(track.id, StemStatus::Running("Stems · Waiting".into()));
    let result = processor.analyze(
        &track.content_hash,
        prepared.analysis.duration_sec,
        || playback::pcm(core, track).map_err(mixless_stems::Error::Model),
        &mut |progress| {
            use mixless_stems::Progress::*;
            let label = match progress {
                Downloading { percent, .. } => format!("Stems · Downloading model · {percent}%"),
                Loading => "Stems · Loading models".into(),
                Separating(p) => format!("Stems · Separating · {p}%"),
                Notes { stem, percent } => format!("Stems · {stem} notes · {percent}%"),
                Saving => "Stems · Saving".into(),
            };
            if active() {
                core.analysis
                    .set_stems(track.id, StemStatus::Running(label));
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
    let evidence = result.map_err(|error| error.to_string())?;
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
        .is_some_and(mixless_stems::compatible_evidence)
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
    core.analysis.set_stems(id, StemStatus::Ready);
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
    fn queued_stem_workers_do_not_block_fresh_basic_analysis() {
        let (dir, mut core, tracks) = crate::automix::tests::fixture();
        let cache = dir.path().join("stems");
        std::fs::create_dir(&cache).unwrap();
        let lock = std::fs::File::create(cache.join(".analysis.lock")).unwrap();
        fs2::FileExt::lock_exclusive(&lock).unwrap();
        Arc::get_mut(&mut core).unwrap().stems = Some(mixless_stems::Processor::new(
            dir.path().join("missing-models"),
            cache,
            false,
        ));
        let workers = core.stems.as_ref().unwrap().parallelism();
        for id in tracks.iter().take(workers) {
            schedule(&core, *id);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let running = core
                .analysis
                .stem_statuses()
                .values()
                .filter(|status| matches!(status, StemStatus::Running(_)))
                .count();
            if running == workers {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "independent stem workers did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let id = tracks[0];
        core.library.reset_analysis(id, false).unwrap();
        core.analysis
            .prepared
            .lock()
            .unwrap()
            .retain(|p| p.track.id != id);
        core.analysis.set(id, Status::Queued);
        let worker = core.clone();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(prepare(&worker, id));
        });
        let result = rx.recv_timeout(std::time::Duration::from_secs(5));
        // Release the fixture lock even if the regression times out.
        drop(lock);
        let prepared = result
            .expect("basic analysis waited for stem workers")
            .unwrap();
        assert!(prepared.track.analyzed);
        assert!(matches!(
            core.analysis.status(&prepared.track),
            Status::Ready(_)
        ));
        core.shutting_down.store(true, Ordering::Release);
    }

    #[test]
    fn obsolete_stem_job_cannot_restore_status_after_reanalysis() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let generation = core.analysis.epoch(id).load(Ordering::Acquire);
        core.analysis.forget(id);
        assert!(run(&core, id, generation).is_err());
        assert!(!core.analysis.stem_statuses().contains_key(&id));
    }

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
        job(&core, id);
        let prepared = prepare(&core, id).unwrap();
        assert!(prepared.analysis.stems.is_none());
        assert!(matches!(
            core.analysis.status(&prepared.track),
            Status::Ready(_)
        ));
        assert!(matches!(
            core.analysis.stem_statuses().get(&id),
            Some(StemStatus::Failed(_))
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
        assert_eq!(
            core.analysis.stem_statuses().get(&id),
            Some(&StemStatus::Ready)
        );
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
            assert!(jobs.enqueue(TrackId(i), false, || {}));
        }
        assert_eq!(jobs.queued(), [TrackId(1), TrackId(2), TrackId(3)]);
        assert!(jobs.enqueue(TrackId(3), true, || {}));
        assert_eq!(jobs.queued(), [TrackId(3), TrackId(1), TrackId(2)]);
        // A duplicate schedule does not reorder or duplicate the entry.
        assert!(jobs.enqueue(TrackId(1), false, || {}));
        assert_eq!(jobs.queued(), [TrackId(3), TrackId(1), TrackId(2)]);
        // The job the worker popped is still pending but no longer queued;
        // promoting it must not requeue it.
        let running = jobs.queue.lock().unwrap().pop_front().unwrap();
        assert_eq!(running, TrackId(3));
        assert!(!jobs.enqueue(TrackId(3), true, || {}));
        assert_eq!(jobs.queued(), [TrackId(1), TrackId(2)]);
    }
}
