//! Prepare adjacent transitions, including the wraparound, before AUTO starts.
use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{
    mpsc::{channel, Sender},
    Mutex, OnceLock,
};

#[derive(Default)]
pub struct Preparation {
    playlist: Mutex<Vec<TrackId>>,
    revision: AtomicU64,
    sender: OnceLock<Sender<(u64, Vec<TrackId>)>>,
    plans: Mutex<HashMap<String, mixless_protocol::MixPlan>>,
    order: Mutex<VecDeque<String>>,
    previews: Mutex<Vec<Option<Arc<mixless_protocol::MixPlan>>>>,
    pub preview_revision: AtomicU64,
}

impl Preparation {
    pub fn previews(&self) -> Vec<Option<Arc<mixless_protocol::MixPlan>>> {
        self.previews.lock().expect("plan previews").clone()
    }
    pub(super) fn has_selection(&self) -> bool {
        self.revision.load(Ordering::Acquire) > 0
    }
    pub(super) fn tracks(&self) -> Vec<TrackId> {
        self.playlist.lock().expect("playlist preparation").clone()
    }
}

pub fn prepare_playlist(core: &Arc<AppCore>, tracks: Vec<TrackId>) {
    prepare(core, tracks, false);
}

pub fn refresh_previews(core: &Arc<AppCore>) {
    prepare(core, core.mix_preparation.tracks(), true);
}

fn prepare(core: &Arc<AppCore>, tracks: Vec<TrackId>, force: bool) {
    for id in &tracks {
        crate::analysis::schedule_deep(core, *id);
    }
    let mut current = core
        .mix_preparation
        .playlist
        .lock()
        .expect("playlist preparation");
    if *current == tracks && !force {
        return;
    }
    *current = tracks.clone();
    drop(current);
    let revision = core.mix_preparation.revision.fetch_add(1, Ordering::AcqRel) + 1;
    *core.mix_preparation.previews.lock().expect("plan previews") = vec![None; tracks.len()];
    core.mix_preparation
        .preview_revision
        .fetch_add(1, Ordering::Release);
    let sender = core.mix_preparation.sender.get_or_init(|| {
        let (tx, rx) = channel::<(u64, Vec<TrackId>)>();
        let weak = Arc::downgrade(core);
        std::thread::spawn(move || {
            while let Ok(mut request) = rx.recv() {
                for newer in rx.try_iter() {
                    request = newer;
                }
                let Some(core) = weak.upgrade() else { break };
                let (revision, tracks) = request;
                // Adjacent pair planning is independent; bound it to two
                // workers so analysis and the realtime/UI threads retain CPU.
                let next = std::sync::atomic::AtomicUsize::new(0);
                std::thread::scope(|scope| {
                    for _ in 0..tracks.len().min(2) {
                        let (core, tracks, next) = (&core, &tracks, &next);
                        scope.spawn(move || loop {
                            if core.mix_preparation.revision.load(Ordering::Acquire) != revision {
                                break;
                            }
                            let i = next.fetch_add(1, Ordering::Relaxed);
                            if i >= tracks.len() {
                                break;
                            }
                            let a = crate::analysis::prepare(core, tracks[i]);
                            let b = crate::analysis::prepare(core, tracks[(i + 1) % tracks.len()]);
                            if let (Ok(a), Ok(b)) = (a, b) {
                                let plan = pair(
                                    core,
                                    &a,
                                    &b,
                                    0.,
                                    PerformanceOffset::identity(),
                                    PerformanceOffset::identity(),
                                );
                                let mut previews =
                                    core.mix_preparation.previews.lock().expect("plan previews");
                                if core.mix_preparation.revision.load(Ordering::Acquire) == revision
                                {
                                    previews[i] = plan.ok().map(Arc::new);
                                    core.mix_preparation
                                        .preview_revision
                                        .fetch_add(1, Ordering::Release);
                                }
                            }
                        });
                    }
                });
                // The analysis queue otherwise leaves only the last songs in
                // the PCM cache. Prime the start of the selected playlist too.
                for id in tracks.iter().take(2) {
                    if core.mix_preparation.revision.load(Ordering::Acquire) != revision {
                        break;
                    }
                    if let Ok(track) = crate::analysis::prepare(&core, *id) {
                        let _ = crate::analysis::decode(&core, &track);
                    }
                }
            }
        });
        tx
    });
    let _ = sender.send((revision, tracks));
}

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
    pair_from_entry(core, a, b, earliest, 0., offset_a, offset_b, following)
}

pub(super) fn pair_from_entry(
    core: &AppCore,
    a: &crate::analysis::PreparedTrack,
    b: &crate::analysis::PreparedTrack,
    earliest: f32,
    entry: f32,
    offset_a: PerformanceOffset,
    offset_b: PerformanceOffset,
    following: Option<TrackId>,
) -> Result<mixless_protocol::MixPlan, String> {
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
    let key = format!(
        "{}:{}:{}:{}:{}:{}:{:?}:{:?}:{}:{}:{:?}:{:?}",
        entry.to_bits(),
        core.analysis.content_revision.load(Ordering::Acquire),
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
            .map(|a| (&a.camelot, a.key_confidence, a.tempo.global_bpm))
    );
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
        return Ok(base);
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
    );
    if let Some(error) = &plan.failure_reason {
        return Err(error.clone());
    }
    Ok(plan)
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
) -> mixless_protocol::MixPlan {
    let planner = mixless_mixplan::Planner::with_options(mixless_mixplan::PlannerOptions {
        harmonic_key_shift: true,
        earliest_outgoing_sec: earliest,
        outgoing_entry_sec: entry,
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
