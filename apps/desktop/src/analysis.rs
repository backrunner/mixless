//! Host-side readiness gate. File I/O, hashing and analysis never run on the
//! UI/audio thread. A decoded deck and its analysis share one file revision.
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{
    Arc, Mutex, OnceLock,
    mpsc::{Sender, channel},
    atomic::{AtomicU64, Ordering},
};

use crate::state::AppCore;
use mixless_protocol::{DeckId, Track, TrackAnalysis, TrackId};

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Queued,
    Checking,
    Analyzing,
    Ready(String),
    Failed(String),
}

impl Status {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Checking => "Reading file",
            Self::Analyzing => "Analyzing",
            Self::Ready(_) => "Ready",
            Self::Failed(_) => "Couldn’t analyze",
        }
    }
}

#[derive(Clone)]
pub struct PreparedTrack {
    pub track: Track,
    pub analysis: TrackAnalysis,
    pub wave: Arc<mixless_protocol::Waveform>,
}

#[derive(Default)]
pub struct AnalysisJobs {
    states: Mutex<HashMap<TrackId, Status>>,
    locks: Mutex<HashMap<TrackId, Arc<Mutex<()>>>>,
    sender: OnceLock<Sender<TrackId>>,
    prepared: Mutex<VecDeque<PreparedTrack>>,
    decoded: Mutex<VecDeque<(TrackId, String, Arc<mixless_engine::AudioBuffer>)>>,
    pub revision: AtomicU64,
    pub loaded: Mutex<[Option<PreparedTrack>; 2]>,
}

impl AnalysisJobs {
    pub fn summary(&self) -> String {
        let states = self.states.lock().expect("analysis states");
        let ready = states.values().filter(|s| matches!(s, Status::Ready(_))).count();
        let failed = states.values().filter(|s| matches!(s, Status::Failed(_))).count();
        let pending = states.len() - ready - failed;
        let mut text = if pending > 0 { format!("Preparing {ready}/{} tracks", states.len()) }
            else { format!("{ready} tracks ready") };
        if failed > 0 { text.push_str(&format!(" · {failed} failed")); }
        text
    }

    pub fn status(&self, track: &Track) -> Status {
        match self.states.lock().expect("analysis states").get(&track.id) {
            Some(Status::Ready(hash)) if !track.analyzed || *hash != track.content_hash => {
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
        self.prepared.lock().expect("prepared cache").retain(|p| p.track.id != id);
        self.states.lock().expect("analysis states").remove(&id);
        self.revision.fetch_add(1, Ordering::Release);
    }
}

/// Queue each visible track once; refreshes must not overwrite worker states.
pub fn schedule(core: &std::sync::Arc<AppCore>, tracks: &[Track]) {
    let mut states = core.analysis.states.lock().expect("analysis states");
    let mut pending = Vec::new();
    for track in tracks {
        let stale = matches!(states.get(&track.id), Some(Status::Ready(hash))
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
        let workers = std::thread::available_parallelism().map_or(2, usize::from)
            .saturating_sub(1).clamp(1, 4);
        for _ in 0..workers {
            let rx = rx.clone();
            let core = Arc::downgrade(core);
            std::thread::spawn(move || loop {
                let Ok(id) = rx.lock().expect("analysis queue").recv() else { break };
                let Some(core) = core.upgrade() else { break };
                let _ = prepare(&core, id);
            });
        }
        tx
    });
    for id in pending { let _ = sender.send(id); }
}

/// Coalesce duplicate requests per track, while independent tracks run in parallel.
pub fn prepare(core: &AppCore, id: TrackId) -> Result<PreparedTrack, String> {
    let lock = core.analysis.locks.lock().expect("analysis locks")
        .entry(id).or_default().clone();
    let _track = lock.lock().map_err(|_| "Analysis worker lock poisoned")?;
    let result = prepare_inner(core, id);
    match &result {
        Ok(prepared) => core.analysis.set(id, Status::Ready(prepared.track.content_hash.clone())),
        Err(error) => core.analysis.set(id, Status::Failed(error.clone())),
    }
    result
}

/// A small PCM cache makes recently prepared/next tracks instant without keeping a playlist in RAM.
pub fn decode(core: &AppCore, prepared: &PreparedTrack) -> Result<Arc<mixless_engine::AudioBuffer>, String> {
    let track = &prepared.track;
    if let Some((_, _, buf)) = core.analysis.decoded.lock().expect("decoded cache").iter()
        .find(|(id, hash, _)| *id == track.id && *hash == track.content_hash) {
        return Ok(buf.clone());
    }
    let buf = mixless_engine::decode_file(Path::new(&track.path)).map_err(|e| e.to_string())?;
    if core.library.verified_content_hash(Path::new(&track.path)).map_err(|e| e.to_string())? != track.content_hash {
        return Err("File changed while loading; retry".into());
    }
    remember_audio(core, track, buf.clone());
    Ok(buf)
}

fn remember_audio(core: &AppCore, track: &Track, buf: Arc<mixless_engine::AudioBuffer>) {
    const MAX_BYTES: usize = 256 * 1024 * 1024;
    let mut cache = core.analysis.decoded.lock().expect("decoded cache");
    cache.retain(|(id, _, _)| *id != track.id);
    if buf.samples.len() * 4 > MAX_BYTES { return; }
    cache.push_back((track.id, track.content_hash.clone(), buf));
    while cache.len() > 2 || cache.iter().map(|(_, _, b)| b.samples.len() * 4).sum::<usize>() > MAX_BYTES {
        cache.pop_front();
    }
}

fn prepare_inner(core: &AppCore, id: TrackId) -> Result<PreparedTrack, String> {
    let mut track = core.library.get_track(id).map_err(|e| e.to_string())?;
    let path_string = track.path.clone();
    let path = Path::new(&path_string);
    let hash = core
        .library
        .verified_content_hash(path)
        .map_err(|e| e.to_string())?;
    if let Some(prepared) = core.analysis.prepared.lock().expect("prepared cache").iter()
        .find(|p| p.track.id == id && p.track.content_hash == hash && track.analyzed) {
        return Ok(prepared.clone());
    }
    core.analysis.set(id, Status::Checking);
    if !track.analyzed || track.content_hash != hash {
        // Fill tags/cover only after discovery has published every filename.
        core.library.import_file(path).map_err(|e| e.to_string())?;
        track = core.library.get_track(id).map_err(|e| e.to_string())?;
    }
    let cached = if track.analyzed && hash == track.content_hash {
        core.library
            .load_analysis(id, mixless_analyze::ANALYSIS_VERSION)
            .ok()
            .flatten()
    } else {
        None
    };
    let cached_wave = core.library.load_waveform(id).ok().flatten();
    let (analysis, wave) = if let (Some(analysis), Some(wave)) = (&cached, &cached_wave) {
        (analysis.clone(), Arc::new(wave.clone()))
    } else {
        if cached.is_none() {
            core.library.begin_analysis(id, &hash).map_err(|e| e.to_string())?;
        }
        core.analysis.set(id, Status::Analyzing);
        let buf = mixless_engine::decode_file(path).map_err(|e| e.to_string())?;
        let (analysis, wave) = std::thread::scope(|scope| {
            let wave = scope.spawn(|| {
                cached_wave.unwrap_or_else(|| {
                    let columns = ((buf.frames / 64) as usize).clamp(4096, 524_288);
                    mixless_engine::compute_waveform(&buf, columns)
                })
            });
            let analysis = cached.unwrap_or_else(|| core.analyzer.analyze_buffer(id, &buf).0);
            (analysis, Arc::new(wave.join().expect("waveform worker")))
        });
        if core.library.verified_content_hash(path).map_err(|e| e.to_string())? != hash {
            return Err("File changed during analysis; retry".into());
        }
        core.library.finish_analysis(&analysis, &hash, mixless_analyze::ANALYSIS_VERSION)
            .map_err(|e| e.to_string())?;
        core.library.save_waveform(id, &hash, &wave).map_err(|e| e.to_string())?;
        remember_audio(core, &track, buf);
        (analysis, wave)
    };
    track = core.library.get_track(id).map_err(|e| e.to_string())?;
    if track.content_hash != hash || !track.analyzed {
        return Err("Track revision changed during analysis".into());
    }
    let prepared = PreparedTrack { track, analysis, wave };
    let mut cache = core.analysis.prepared.lock().expect("prepared cache");
    cache.retain(|p| p.track.id != id);
    cache.push_back(prepared.clone());
    while cache.len() > 8 { cache.pop_front(); }
    Ok(prepared)
}

/// Verify the decoded revision before the short publication lock. Cancelling
/// AUTO must never wait for filesystem I/O on the UI thread.
pub fn load(
    core: &AppCore,
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
    if core.library.verified_content_hash(Path::new(&track.path)).map_err(|e| e.to_string())? != track.content_hash {
        core.analysis.forget(id);
        return Err("File changed while loading; retry".into());
    }
    let cues = core.library.cues(id).map_err(|e| e.to_string())?;
    let _load = core.deck_load.lock().map_err(|_| "Deck load mutex poisoned")?;
    let _commit = core.automix_commit.lock().map_err(|_| "Automix commit lock poisoned")?;
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
    if prepared.analysis.tempo.beats.len() >= 2 {
        core.engine
            .set_beat_grid(deck, id, prepared.analysis.tempo.clone())
            .map_err(|e| e.to_string())?;
    }
    core.engine
        .set_bpm(deck, prepared.analysis.tempo.global_bpm);
    for cue in cues {
        core.engine.set_cue_frame(deck, cue.index, cue.frame);
    }
    core.analysis.loaded.lock().expect("loaded analysis")[deck.index()] = Some(prepared);
    Ok(true)
}
