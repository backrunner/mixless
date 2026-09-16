//! Host-side readiness gate. File I/O, hashing and analysis never run on the
//! UI/audio thread. A decoded deck and its analysis share one file revision.
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
    mpsc::{Sender, channel},
};

mod deep;
mod playback;
mod reanalysis;
mod stems;
pub use deep::promote as promote_deep;
pub use deep::schedule as schedule_deep;
pub use playback::load_manual;
pub use reanalysis::reanalyze;
pub use stems::attach_stems;

use crate::state::AppCore;
use mixless_protocol::{DeckId, Track, TrackAnalysis, TrackId};

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Queued,
    Checking,
    Analyzing,
    Enhancing(String),
    Ready(String),
    Basic(String, String),
    Failed(String),
}

#[derive(Clone)]
pub struct PreparedTrack {
    pub track: Track,
    pub analysis: Arc<TrackAnalysis>,
    pub wave: Arc<mixless_protocol::Waveform>,
    pub warning: Option<String>,
}

#[derive(Default)]
pub struct AnalysisJobs {
    states: Mutex<HashMap<TrackId, Status>>,
    locks: Mutex<HashMap<TrackId, Arc<Mutex<()>>>>,
    audio_locks: Mutex<HashMap<TrackId, Arc<Mutex<()>>>>,
    epochs: Mutex<HashMap<TrackId, Arc<AtomicU64>>>,
    deep_jobs: deep::Jobs,
    updates: Mutex<HashMap<TrackId, Track>>,
    pub latest: Mutex<HashMap<TrackId, Track>>,
    sender: OnceLock<Sender<TrackId>>,
    prepared: Mutex<VecDeque<PreparedTrack>>,
    decoded: Mutex<VecDeque<(TrackId, String, Arc<mixless_engine::AudioBuffer>)>>,
    pub revision: AtomicU64,
    pub content_revision: AtomicU64,
    pub loaded: Mutex<[Option<PreparedTrack>; 2]>,
    playback_revision: Mutex<[Option<(std::sync::Weak<mixless_engine::AudioBuffer>, String)>; 2]>,
}

impl AnalysisJobs {
    pub fn summary(&self) -> String {
        let states = self.states.lock().expect("analysis states");
        let ready = states
            .values()
            .filter(|s| {
                matches!(
                    s,
                    Status::Ready(_) | Status::Basic(_, _) | Status::Enhancing(_)
                )
            })
            .count();
        let failed = states
            .values()
            .filter(|s| matches!(s, Status::Failed(_)))
            .count();
        let pending = states.len() - ready - failed;
        let mut text = if pending > 0 {
            format!(
                "Analyzing · {ready}/{} complete · {pending} remaining",
                states.len()
            )
        } else {
            format!("{ready} analyzed")
        };
        if failed > 0 {
            text.push_str(&format!(" · {failed} failed"));
        }
        let basic = states
            .values()
            .filter(|s| matches!(s, Status::Basic(_, _)))
            .count();
        if basic > 0 {
            text.push_str(&format!(" · {basic} using basic analysis"));
        }
        let deep = self.deep_jobs.pending.lock().expect("deep jobs").len();
        if deep > 0 {
            text.push_str(&format!(" · {deep} stem analyses queued / running"));
        }
        text
    }

    pub fn take_updates(&self) -> HashMap<TrackId, Track> {
        std::mem::take(&mut *self.updates.lock().expect("analysis updates"))
    }

    /// Queue a track row refresh after an out-of-band library write
    /// (e.g. storage cleanup cleared its analysis).
    pub fn updated(&self, track: Track) {
        self.updates
            .lock()
            .expect("analysis updates")
            .insert(track.id, track);
    }

    pub fn statuses(&self) -> HashMap<TrackId, Status> {
        self.states.lock().expect("analysis states").clone()
    }

    pub fn status(&self, track: &Track) -> Status {
        match self.states.lock().expect("analysis states").get(&track.id) {
            Some(Status::Ready(hash) | Status::Basic(hash, _))
                if !track.analyzed || *hash != track.content_hash =>
            {
                Status::Checking
            }
            Some(status) => status.clone(),
            None => Status::Checking,
        }
    }

    fn set(&self, id: TrackId, status: Status) {
        let mut states = self.states.lock().expect("analysis states");
        if states.get(&id) != Some(&status) {
            states.insert(id, status);
            self.revision.fetch_add(1, Ordering::Release);
        }
    }

    pub fn forget(&self, id: TrackId) {
        self.epoch(id).fetch_add(1, Ordering::AcqRel);
        self.prepared
            .lock()
            .expect("prepared cache")
            .retain(|p| p.track.id != id);
        self.states.lock().expect("analysis states").remove(&id);
        self.latest.lock().expect("analysis metadata").remove(&id);
        self.updates.lock().expect("analysis updates").remove(&id);
        self.revision.fetch_add(1, Ordering::Release);
    }

    fn epoch(&self, id: TrackId) -> Arc<AtomicU64> {
        self.epochs
            .lock()
            .expect("analysis epochs")
            .entry(id)
            .or_default()
            .clone()
    }
}

/// Queue each visible track once; refreshes must not overwrite worker states.
pub fn schedule(core: &std::sync::Arc<AppCore>, tracks: &[Track]) {
    let mut states = core.analysis.states.lock().expect("analysis states");
    let mut pending = Vec::new();
    for track in tracks {
        let stale = matches!(states.get(&track.id), Some(Status::Ready(hash) | Status::Basic(hash, _))
            if !track.analyzed || hash != &track.content_hash);
        if !states.contains_key(&track.id) || stale {
            states.insert(track.id, Status::Queued);
            pending.push(track.id);
        }
    }
    drop(states);
    if pending.is_empty() {
        return;
    }
    let sender = core.analysis.sender.get_or_init(|| {
        let (tx, rx) = channel();
        let rx = Arc::new(Mutex::new(rx));
        // Keep one core available for the audio callback/UI while allowing
        // several independent tracks to decode and analyze at once.
        let workers = std::thread::available_parallelism()
            .map_or(2, usize::from)
            .saturating_sub(1)
            .clamp(1, 4);
        for _ in 0..workers {
            let rx = rx.clone();
            let core = Arc::downgrade(core);
            std::thread::spawn(move || {
                loop {
                    let Ok(id) = rx.lock().expect("analysis queue").recv() else {
                        break;
                    };
                    let Some(core) = core.upgrade() else { break };
                    if prepare(&core, id).is_ok() {
                        deep::schedule(&core, id);
                    }
                }
            });
        }
        tx
    });
    for id in pending {
        let _ = sender.send(id);
    }
}

/// Coalesce duplicate requests per track, while independent tracks run in parallel.
pub fn prepare(core: &AppCore, id: TrackId) -> Result<PreparedTrack, String> {
    let lock = core
        .analysis
        .locks
        .lock()
        .expect("analysis locks")
        .entry(id)
        .or_default()
        .clone();
    let _track = lock.lock().map_err(|_| "Analysis worker lock poisoned")?;
    let result = prepare_inner(core, id);
    match &result {
        Ok(prepared) => {
            if core.analysis.status(&prepared.track)
                != Status::Ready(prepared.track.content_hash.clone())
            {
                core.analysis
                    .latest
                    .lock()
                    .expect("analysis metadata")
                    .insert(id, prepared.track.clone());
                core.analysis
                    .updates
                    .lock()
                    .expect("analysis updates")
                    .insert(id, prepared.track.clone());
            }
            if !core
                .analysis
                .deep_jobs
                .pending
                .lock()
                .expect("deep jobs")
                .contains(&id)
                && !matches!(core.analysis.status(&prepared.track), Status::Basic(_, _))
            {
                core.analysis.set(id, deep::ready_status(prepared));
            }
        }
        Err(error) => core.analysis.set(id, Status::Failed(error.clone())),
    }
    result
}

/// A small PCM cache makes recently prepared/next tracks instant without keeping a playlist in RAM.
pub fn decode(
    core: &AppCore,
    prepared: &PreparedTrack,
) -> Result<Arc<mixless_engine::AudioBuffer>, String> {
    playback::audio(core, &prepared.track).map(|audio| audio.buf)
}

fn remember_audio(core: &AppCore, track: &Track, buf: Arc<mixless_engine::AudioBuffer>) {
    const MAX_BYTES: usize = 256 * 1024 * 1024;
    let mut cache = core.analysis.decoded.lock().expect("decoded cache");
    cache.retain(|(id, _, _)| *id != track.id);
    if buf.samples.len() * 4 > MAX_BYTES {
        return;
    }
    cache.push_back((track.id, track.content_hash.clone(), buf));
    while cache.len() > 2
        || cache
            .iter()
            .map(|(_, _, b)| b.samples.len() * 4)
            .sum::<usize>()
            > MAX_BYTES
    {
        cache.pop_front();
    }
}

fn prepare_inner(core: &AppCore, id: TrackId) -> Result<PreparedTrack, String> {
    let track = playback::current_track(core, id)?;
    let path = Path::new(&track.path);
    let hash = track.content_hash.clone();
    if let Some(prepared) = core
        .analysis
        .prepared
        .lock()
        .expect("prepared cache")
        .iter()
        .find(|p| p.track.id == id && p.track.content_hash == hash && track.analyzed)
    {
        return Ok(prepared.clone());
    }
    // Cache hits must not flicker a finished row: only a real decode/analysis
    // below may publish a busy status over Ready.
    let cached = if track.analyzed && hash == track.content_hash {
        core.library
            .load_analysis(id, mixless_analyze::ANALYSIS_VERSION)
            .ok()
            .flatten()
    } else {
        None
    };
    let refresh_cues = cached.is_none();
    let cached_wave = core.library.load_waveform(id).ok().flatten();
    let (analysis, wave) = if let (Some(analysis), Some(wave)) = (&cached, &cached_wave) {
        (analysis.clone(), Arc::new(wave.clone()))
    } else {
        if cached.is_none() {
            core.library
                .begin_analysis(id, &hash)
                .map_err(|e| e.to_string())?;
        }
        core.analysis.set(id, Status::Analyzing);
        // Publish the spectral cache before musical analysis. A manual load
        // can now use it while this track's beat/phrase analysis is still running.
        let audio = playback::audio(core, &track)?;
        let analysis = cached.unwrap_or_else(|| core.analyzer.analyze_buffer(id, &audio.buf).0);
        let wave = audio.wave;
        if core
            .library
            .verified_content_hash(path)
            .map_err(|e| e.to_string())?
            != hash
        {
            return Err("File changed during analysis; retry".into());
        }
        core.library
            .finish_analysis(&analysis, &hash, mixless_analyze::ANALYSIS_VERSION)
            .map_err(|e| e.to_string())?;
        core.library
            .save_waveform(id, &hash, &wave)
            .map_err(|e| e.to_string())?;
        (analysis, wave)
    };
    let track = core.library.get_track(id).map_err(|e| e.to_string())?;
    if track.content_hash != hash || !track.analyzed {
        return Err("Track revision changed during analysis".into());
    }
    if refresh_cues
        || !core
            .library
            .automatic_cues_current(id)
            .map_err(|e| e.to_string())?
    {
        core.library
            .replace_auto_cues(id, &mixless_analyze::automatic_cues(&analysis))
            .map_err(|e| e.to_string())?;
    }
    let prepared = PreparedTrack {
        track,
        analysis: Arc::new(analysis),
        wave,
        warning: None,
    };
    let mut cache = core.analysis.prepared.lock().expect("prepared cache");
    cache.retain(|p| p.track.id != id);
    cache.push_back(prepared.clone());
    while cache.len() > 8 {
        cache.pop_front();
    }
    Ok(prepared)
}

/// Verify the decoded revision before the short publication lock. Cancelling
/// AUTO must never wait for filesystem I/O on the UI thread.
pub fn load(
    core: &Arc<AppCore>,
    deck: DeckId,
    id: TrackId,
    active: impl Fn() -> bool,
) -> Result<bool, String> {
    if !active() {
        return Ok(false);
    }
    let prepared = prepare(core, id)?;
    if !active() {
        return Ok(false);
    }
    let buf = decode(core, &prepared)?;
    let track = &prepared.track;
    if core
        .library
        .verified_content_hash(Path::new(&track.path))
        .map_err(|e| e.to_string())?
        != track.content_hash
    {
        core.analysis.forget(id);
        return Err("File changed while loading; retry".into());
    }
    let _load = core
        .deck_load
        .lock()
        .map_err(|_| "Deck load mutex poisoned")?;
    let _commit = core
        .automix_commit
        .lock()
        .map_err(|_| "Automix commit lock poisoned")?;
    let source = Arc::downgrade(&buf);
    let committed = core
        .engine
        .load_buffer_if(
            deck,
            id,
            buf,
            prepared.wave.clone(),
            track.title.clone(),
            track.artist.clone(),
            &active,
        )
        .map_err(|e| e.to_string())?;
    if !committed {
        if active() {
            core.analysis.forget(id);
            return Err("File changed while loading; analysis will be refreshed".into());
        }
        return Ok(false);
    }
    core.analysis
        .playback_revision
        .lock()
        .expect("source revision")[deck.index()] = Some((source, track.content_hash.clone()));
    if prepared.analysis.tempo.beats.len() >= 2 {
        core.engine
            .set_beat_grid(deck, id, prepared.analysis.tempo.clone())
            .map_err(|e| e.to_string())?;
    }
    core.engine
        .set_bpm(deck, prepared.analysis.tempo.global_bpm);
    for cue in core.library.cues(id).map_err(|e| e.to_string())? {
        core.engine.set_cue_frame(deck, cue.index, cue.frame);
        let _ = core.engine.dispatch(mixless_protocol::Command::SetCueKind {
            track_id: id,
            index: cue.index,
            kind: if cue.user_set {
                cue.kind
            } else {
                mixless_protocol::CueKind::Hot
            },
        });
    }
    let hash = prepared.track.content_hash.clone();
    core.analysis.loaded.lock().expect("loaded analysis")[deck.index()] = Some(prepared);
    drop(_commit);
    drop(_load);
    attach_stems(core, deck, id, &hash);
    deep::promote(core, id);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_hit_revalidation_does_not_flicker_ready_status() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let hash = core.library.get_track(id).unwrap().content_hash;
        // Drop the in-memory prepared entry so the next prepare revalidates
        // from SQLite, like an automix sweep past the 8-entry cache.
        core.analysis
            .prepared
            .lock()
            .expect("prepared cache")
            .retain(|p| p.track.id != id);
        let before = core.analysis.revision.load(Ordering::Acquire);
        let prepared = prepare(&core, id).unwrap();
        assert_eq!(prepared.track.content_hash, hash);
        assert_eq!(core.analysis.status(&prepared.track), Status::Ready(hash));
        assert_eq!(core.analysis.revision.load(Ordering::Acquire), before);
    }

    #[test]
    fn cache_hit_revalidation_keeps_basic_status() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let hash = core.library.get_track(id).unwrap().content_hash;
        core.analysis
            .set(id, Status::Basic(hash, "stem analysis failed".into()));
        core.analysis
            .prepared
            .lock()
            .expect("prepared cache")
            .retain(|p| p.track.id != id);
        prepare(&core, id).unwrap();
        assert!(matches!(
            core.analysis.status(&core.library.get_track(id).unwrap()),
            Status::Basic(_, _)
        ));
    }

    #[test]
    fn cached_analysis_does_not_recreate_a_deleted_suggestion() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let index = core.library.cues(id).unwrap()[0].index;
        core.library.clear_cue(id, index).unwrap();
        core.analysis.forget(id);
        prepare(&core, id).unwrap();
        assert!(
            core.library
                .cues(id)
                .unwrap()
                .iter()
                .all(|c| c.index != index)
        );
    }

    #[test]
    fn manual_load_does_not_wait_for_the_musical_analysis_lock() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        let id = tracks[0];
        let hash = core.library.get_track(id).unwrap().content_hash;
        core.library.begin_analysis(id, &hash).unwrap();
        core.analysis.forget(id);
        let lock = core
            .analysis
            .locks
            .lock()
            .unwrap()
            .entry(id)
            .or_default()
            .clone();
        let guard = lock.lock().unwrap();
        let (tx, rx) = channel();
        let worker_core = core.clone();
        let worker = std::thread::spawn(move || {
            let _ = tx.send(load_manual(&worker_core, DeckId::A, id, || true));
        });
        let result = rx.recv_timeout(std::time::Duration::from_secs(2));
        drop(guard);
        worker.join().unwrap();
        assert!(result.unwrap().unwrap().is_some());
        assert_eq!(core.engine.snapshot().decks[0].track_id, Some(id));
        assert!(!core.library.get_track(id).unwrap().analyzed);
    }

    #[test]
    fn superseded_manual_load_cannot_publish_after_decode() {
        let (_dir, core, tracks) = crate::automix::tests::fixture();
        load_manual(&core, DeckId::A, tracks[0], || true).unwrap();
        let checks = std::sync::atomic::AtomicUsize::new(0);
        assert!(
            load_manual(&core, DeckId::A, tracks[1], || checks
                .fetch_add(1, Ordering::Relaxed)
                == 0)
            .unwrap()
            .is_none()
        );
        assert_eq!(core.engine.snapshot().decks[0].track_id, Some(tracks[0]));
        load_manual(&core, DeckId::B, tracks[2], || true).unwrap();
        assert_eq!(core.engine.snapshot().decks[1].track_id, Some(tracks[2]));
    }
}
