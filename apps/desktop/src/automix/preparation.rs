//! Prepare adjacent transitions, including the wraparound, before AUTO starts.
use super::*;
use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock, mpsc::{Sender, channel}};

#[derive(Default)]
pub struct Preparation {
    playlist: Mutex<Vec<TrackId>>,
    revision: AtomicU64,
    sender: OnceLock<Sender<(u64, Vec<TrackId>)>>,
    plans: Mutex<HashMap<String, mixless_protocol::MixPlan>>,
    order: Mutex<VecDeque<String>>,
}

pub fn prepare_playlist(core: &Arc<AppCore>, tracks: Vec<TrackId>) {
    let mut current = core.mix_preparation.playlist.lock().expect("playlist preparation");
    if *current == tracks { return; }
    *current = tracks.clone();
    drop(current);
    let revision = core.mix_preparation.revision.fetch_add(1, Ordering::AcqRel) + 1;
    let sender = core.mix_preparation.sender.get_or_init(|| {
        let (tx, rx) = channel::<(u64, Vec<TrackId>)>();
        let weak = Arc::downgrade(core);
        std::thread::spawn(move || {
            while let Ok(mut request) = rx.recv() {
                for newer in rx.try_iter() { request = newer; }
                let Some(core) = weak.upgrade() else { break };
                let (revision, tracks) = request;
                for i in 0..tracks.len() {
                    if core.mix_preparation.revision.load(Ordering::Acquire) != revision { break; }
                    let a = crate::analysis::prepare(&core, tracks[i]);
                    let b = crate::analysis::prepare(&core, tracks[(i + 1) % tracks.len()]);
                    if let (Ok(a), Ok(b)) = (a, b) {
                        let _ = pair(&core, &a, &b, 0., PerformanceOffset::identity(), PerformanceOffset::identity());
                    }
                }
            }
        });
        tx
    });
    let _ = sender.send((revision, tracks));
}

pub(super) fn pair(
    core: &AppCore, a: &crate::analysis::PreparedTrack, b: &crate::analysis::PreparedTrack,
    earliest: f32, offset_a: PerformanceOffset, offset_b: PerformanceOffset,
) -> Result<mixless_protocol::MixPlan, String> {
    let ca = core.library.cues(a.track.id).map_err(|e| e.to_string())?;
    let cb = core.library.cues(b.track.id).map_err(|e| e.to_string())?;
    let key = format!("{}:{}:{}:{}:{:?}:{:?}:{}:{}", a.track.id.0, b.track.id.0,
        a.track.content_hash, b.track.content_hash, offset_a, offset_b,
        serde_json::to_string(&ca).map_err(|e| e.to_string())?,
        serde_json::to_string(&cb).map_err(|e| e.to_string())?);
    let cached = core.mix_preparation.plans.lock().expect("plans").get(&key).cloned();
    let base = cached.unwrap_or_else(|| {
        let plan = mixless_mixplan::Planner::new().plan_pair(&a.analysis, &b.analysis, &ca, &cb, offset_a, offset_b);
        let mut plans = core.mix_preparation.plans.lock().expect("plans");
        let mut order = core.mix_preparation.order.lock().expect("plan order");
        if !plans.contains_key(&key) { order.push_back(key.clone()); }
        plans.insert(key.clone(), plan.clone());
        while order.len() > 256 { if let Some(old) = order.pop_front() { plans.remove(&old); } }
        plan
    });
    if base.failure_reason.is_none() && base.t_in_a >= earliest { return Ok(base); }
    let ctx = mixless_mixplan::PlanContext { outgoing: &a.analysis, incoming: &b.analysis,
        cues_out: &ca, cues_in: &cb, offset_a, offset_b };
    let mut plan = mixless_mixplan::Planner::with_options(mixless_mixplan::PlannerOptions {
        earliest_outgoing_sec: earliest, ..Default::default()
    }).plan_next(&ctx);
    if plan.failure_reason.is_some() {
        plan = mixless_mixplan::short_handoff(&ctx, earliest);
    }
    if let Some(error) = &plan.failure_reason { return Err(error.clone()); }
    Ok(plan)
}
