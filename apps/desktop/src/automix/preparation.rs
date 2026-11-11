//! Prepare one continuous set and publish its dependent decisions together.
mod order_cache;
mod ordering;
mod pair_cache;
mod preview;
mod queue;
mod selection;
pub use queue::{OrderMode, QueueEntry, QueueView};
#[cfg(test)]
mod queue_tests;
use super::*;
use ordering::{OrderingJob, arrange};
pub use ordering::{arranged, order_ready};
use preview::Sequence;
pub(super) use queue::Intent;
pub(super) use queue::entries;
#[cfg(test)]
pub use selection::set_best;
pub use selection::set_order_mode;
pub use selection::{default_to_best, prefer_cached_best, reset_order_for_playlist};
use std::collections::{HashMap, VecDeque};
use std::sync::{
    Mutex, OnceLock,
    mpsc::{Sender, channel},
};

#[derive(Clone)]
pub(super) struct ScheduledPair {
    pub key: String,
    pub plan: Arc<mixless_protocol::MixPlan>,
}

pub struct Preparation {
    playlist: Mutex<Vec<TrackId>>,
    selected: Mutex<Vec<TrackId>>,
    revision: AtomicU64,
    input_revision: AtomicU64,
    sender: OnceLock<Sender<(u64, Vec<TrackId>)>>,
    pub(super) plans: Mutex<HashMap<String, mixless_protocol::MixPlan>>,
    pub(super) order: Mutex<VecDeque<String>>,
    sequence: Mutex<Sequence>,
    pub preview_revision: AtomicU64,
    best: std::sync::atomic::AtomicBool,
    /// Warm Best independently of the order used by this AUTO session.
    warming: std::sync::atomic::AtomicBool,
    /// Startup waits only for a cache lookup, never the optimizer.
    defaulting: std::sync::atomic::AtomicBool,
    /// Last displayed order and the evidence revision it was validated against.
    arranged: Mutex<Option<order_cache::Arranged>>,
    /// The playlist a worker is arranging now, so it is measured only once.
    arranging: Mutex<Option<Arc<OrderingJob>>>,
    /// The current source has an explicit order choice.
    chosen: std::sync::atomic::AtomicBool,
}

impl Default for Preparation {
    fn default() -> Self {
        Self {
            playlist: Mutex::default(),
            selected: Mutex::default(),
            revision: AtomicU64::default(),
            input_revision: AtomicU64::default(),
            sender: OnceLock::default(),
            plans: Mutex::default(),
            order: Mutex::default(),
            sequence: Mutex::default(),
            preview_revision: AtomicU64::default(),
            best: std::sync::atomic::AtomicBool::new(false),
            warming: std::sync::atomic::AtomicBool::new(false),
            defaulting: std::sync::atomic::AtomicBool::new(false),
            chosen: std::sync::atomic::AtomicBool::new(false),
            arranged: Mutex::default(),
            arranging: Mutex::default(),
        }
    }
}

impl Preparation {
    pub(super) fn scheduled(&self, key: &str, earliest: f32) -> Option<ScheduledPair> {
        self.sequence
            .lock()
            .expect("set plan")
            .pairs
            .iter()
            .flatten()
            .find(|p| p.key == key && p.plan.t_in_a >= earliest)
            .cloned()
    }
    pub(super) fn has_selection(&self) -> bool {
        self.revision.load(Ordering::Acquire) > 0
    }
    pub fn tracks(&self) -> Vec<TrackId> {
        self.playlist.lock().expect("playlist preparation").clone()
    }
    pub(super) fn revision(&self) -> u64 {
        self.input_revision.load(Ordering::Acquire)
    }
}

pub(super) fn following(
    core: &Arc<AppCore>,
    incoming: TrackId,
    order: &order::TrackOrder,
    count: usize,
    shuffle: bool,
) {
    if core.mix_preparation.tracks().is_empty() {
        return;
    }
    let Some(entry) = order.current_entry().filter(|e| e.track == incoming) else {
        return;
    };
    let mut queue = vec![entry];
    queue.extend(order.preview_entries(count, shuffle));
    if queue.len() > 1 && queue.last() == Some(&entry) {
        queue.pop();
    }
    let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
    // Also refresh an unchanged circuit after its committed entrance changes.
    // This runs after the worker publishes the exact active occurrences.
    sequence.following = Some(queue);
    drop(sequence);
    prepare(core, core.mix_preparation.tracks(), true, false);
}

pub fn prepare_playlist(core: &Arc<AppCore>, tracks: Vec<TrackId>) {
    let mut selected = core
        .mix_preparation
        .selected
        .lock()
        .expect("selected playlist");
    *selected = tracks.clone();
    let arranged = arrange(core, tracks.clone());
    if core.mix_preparation.best() && tracks.len() >= 3 {
        if ordering::arranged(core, &tracks).is_none() && !core.mix_preparation.tracks().is_empty()
        {
            return;
        }
        if ordering::arranged(core, &tracks).is_some() {
            core.mix_preparation.best_ready();
        }
    }
    prepare(core, arranged, false, true);
}

pub fn refresh_previews(core: &Arc<AppCore>) {
    prepare(core, core.mix_preparation.tracks(), true, true);
}

pub(super) fn reset_session(core: &Arc<AppCore>) {
    let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
    sequence.following = None;
    sequence.anchor_entries = None;
    let changed = sequence.anchor.take().is_some();
    if changed {
        core.mix_preparation.revision.fetch_add(1, Ordering::AcqRel);
        core.mix_preparation
            .preview_revision
            .fetch_add(1, Ordering::Release);
    }
    drop(sequence);
    if changed {
        prepare(core, core.mix_preparation.tracks(), true, false);
    }
}

/// Called only after successful engine publication, under automix_commit.
pub(super) fn committed(core: &Arc<AppCore>, plan: Arc<mixless_protocol::MixPlan>) {
    let tracks = core.mix_preparation.tracks();
    if tracks.is_empty() {
        return;
    }
    let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
    sequence.commit(&tracks, plan);
    core.mix_preparation.revision.fetch_add(1, Ordering::AcqRel);
    core.mix_preparation
        .preview_revision
        .fetch_add(1, Ordering::Release);
    drop(sequence);
    prepare(core, core.mix_preparation.tracks(), true, false);
}

fn adjacent_index(tracks: &[TrackId], pair: Option<(TrackId, TrackId)>) -> Option<usize> {
    let pair = pair?;
    (0..tracks.len()).find(|&i| (tracks[i], tracks[(i + 1) % tracks.len()]) == pair)
}

fn prepare(core: &Arc<AppCore>, tracks: Vec<TrackId>, force: bool, input: bool) {
    for id in &tracks {
        crate::analysis::schedule_deep(core, *id);
    }
    for id in tracks.iter().take(2) {
        crate::analysis::promote_deep(core, *id);
    }
    let mut current = core
        .mix_preparation
        .playlist
        .lock()
        .expect("playlist preparation");
    let changed = *current != tracks;
    let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
    let mode_changed = sequence.queue.prepared_mode != sequence.queue.target.mode;
    if !changed && !force && !mode_changed {
        return;
    }
    let input_revision = if input {
        core.mix_preparation
            .input_revision
            .fetch_add(1, Ordering::AcqRel)
            + 1
    } else {
        core.mix_preparation.revision()
    };
    *current = tracks.clone();
    sequence.queue.prepared_mode = sequence.queue.target.mode;
    sequence.queue.target.revision = input_revision;
    let revision = core.mix_preparation.revision.fetch_add(1, Ordering::AcqRel) + 1;
    if changed {
        sequence.select(&tracks);
        core.mix_preparation
            .preview_revision
            .fetch_add(1, Ordering::Release);
    }
    drop(sequence);
    drop(current);
    // Analysis completions coalesce. A same-set refresh keeps the last complete
    // forecast visible, rather than blanking and republishing each pair.
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
                let active = || core.mix_preparation.revision.load(Ordering::Acquire) == revision;
                let Some(pairs) = plan_sequence(&core, &tracks, &active) else {
                    continue;
                };
                if !core.mix_preparation.publish_previews(revision, pairs) {
                    continue;
                }
                for id in tracks.iter().take(2) {
                    if !active() {
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

fn stem_playback_ready(core: &AppCore, track: &crate::analysis::PreparedTrack) -> bool {
    core.stems.as_ref().is_some_and(|processor| {
        let Ok(hash) = core
            .library
            .stem_hash(std::path::Path::new(&track.track.path))
        else {
            return false;
        };
        processor
            .cached(&hash, track.analysis.duration_sec)
            .ok()
            .flatten()
            .as_ref()
            .is_some_and(mixless_stems::is_current)
    })
}

// One second of wall-clock staging room after the prior mix releases the
// other deck. This is an orchestration reserve, not a musical boundary rule.
const STAGING_SECONDS: f32 = 1.;

pub(super) fn plan_sequence(
    core: &Arc<AppCore>,
    tracks: &[TrackId],
    active: &impl Fn() -> bool,
) -> Option<Vec<Option<ScheduledPair>>> {
    let (following, anchor, anchor_entries) = {
        let sequence = core.mix_preparation.sequence.lock().expect("set plan");
        (
            sequence.following.clone(),
            sequence.anchor.clone(),
            sequence.anchor_entries,
        )
    };
    let rows = following.clone().unwrap_or_else(|| entries(tracks));
    let following_tracks: Option<Vec<_>> = following
        .as_ref()
        .map(|rows| rows.iter().map(|e| e.track).collect());
    let tracks = following_tracks.as_deref().unwrap_or(tracks);
    let mut pairs = vec![None; tracks.len()];
    if tracks.len() < 2 {
        return Some(pairs);
    }
    let exact_pair = anchor_entries.filter(|p| {
        anchor
            .as_ref()
            .and_then(|a| a.summary.as_ref())
            .map(|s| s.pair)
            == Some((p[0].track, p[1].track))
    });
    if anchor.is_some() && exact_pair.is_none() && rows.iter().any(|r| r.occurrence > 0) {
        // Wait for playback acknowledgement instead of guessing which copy is
        // sounding. `following` schedules another forecast after publication.
        return None;
    }
    let anchor_index = if let Some(pair) = exact_pair {
        (0..rows.len()).find(|i| [rows[*i], rows[(*i + 1) % rows.len()]] == pair)
    } else {
        adjacent_index(
            tracks,
            anchor
                .as_ref()
                .and_then(|p| p.summary.as_ref().map(|s| s.pair)),
        )
    };
    // Snapshot all analyses first, so following-track evidence is available
    // for the very first decision as well as the last one.
    let mut prepared = Vec::with_capacity(tracks.len());
    let mut stems = Vec::with_capacity(tracks.len());
    for id in tracks {
        if !active() {
            return None;
        }
        let track = crate::analysis::prepare(core, *id).ok();
        let ready = track.as_ref().is_some_and(|t| stem_playback_ready(core, t));
        prepared.push(track);
        stems.push(ready);
    }
    let root = exact_pair.map(|p| p[0]).or_else(|| {
        let track = anchor.as_ref()?.summary.as_ref()?.pair.0;
        rows.iter().find(|entry| entry.track == track).copied()
    });
    let (first, count, mut entry, mut earliest, mut offset_a) = if let Some(plan) = anchor {
        let pair = plan.summary.as_ref()?.pair;
        let incoming = if let Some(exact) = exact_pair {
            rows.iter().position(|entry| *entry == exact[1])
        } else {
            tracks.iter().position(|id| *id == pair.1)
        };
        let Some(incoming) = incoming else {
            return Some(pairs);
        };
        let cursor = (
            prepared[incoming].as_ref().map_or(plan.t_end_b, |track| {
                mixless_mixplan::established_entry(&plan, &track.analysis)
            }),
            plan.t_end_b,
            plan.incoming_offset_end,
        );
        if let Some(i) = anchor_index {
            pairs[i] = Some(ScheduledPair {
                key: String::new(),
                plan,
            });
        }
        // A reorder or shuffle can make the armed pair non-adjacent. Continue
        // from its actual incoming track, never from the nominal old neighbour.
        (incoming, tracks.len() - 1, cursor.0, cursor.1, cursor.2)
    } else {
        let entry = prepared[0]
            .as_ref()
            .and_then(|t| {
                cue_frame(core, t)
                    .ok()
                    .map(|frame| frame as f32 / t.analysis.sample_rate.max(1) as f32)
            })
            .unwrap_or(0.);
        (
            0,
            tracks.len() - 1,
            entry,
            entry,
            PerformanceOffset::identity(),
        )
    };
    for step in 0..count {
        if !active() {
            return None;
        }
        let i = (first + step) % tracks.len();
        let j = (i + 1) % tracks.len();
        if Some(rows[j]) == root {
            break;
        }
        let (Some(a), Some(b)) = (&prepared[i], &prepared[j]) else {
            break;
        };
        // A future set cannot pretend a manual OUT has already been missed.
        // Leave the dependent suffix unplanned if the incoming mix consumes it.
        let ready = earliest + STAGING_SECONDS * offset_a.rate;
        if core.library.cues(a.track.id).ok()?.iter().any(|cue| {
            cue.user_set
                && cue.kind == mixless_protocol::CueKind::Out
                && cue.frame as f32 / a.analysis.sample_rate.max(1) as f32 <= ready
        }) {
            break;
        }
        let pair = super::pair::scheduled_pair(
            core,
            a,
            b,
            ready,
            entry,
            offset_a,
            PerformanceOffset::identity(),
            Some(tracks[(j + 1) % tracks.len()]),
            stems[i] && stems[j],
        );
        let pair = match pair {
            Ok(pair) => pair,
            Err(error) => {
                tracing::debug!(outgoing = a.track.id.0, incoming = b.track.id.0, %error, "Set planning stopped at unavailable transition");
                #[cfg(test)]
                eprintln!(
                    "Set planning {} -> {} stopped: {error}; entry={entry} ready={ready} offset={offset_a:?}",
                    a.track.title, b.track.title
                );
                break;
            }
        };
        #[cfg(test)]
        if std::env::var_os("MIXLESS_SET_AUDIT_DB").is_some() {
            eprintln!(
                "Prepared {} -> {}: {:?}, {:.3}..{:.3} -> {:.3}..{:.3}, {:?}",
                a.track.title,
                b.track.title,
                pair.plan.summary.as_ref().map(|s| s.strategy),
                pair.plan.t_in_a,
                pair.plan.t_out_a,
                pair.plan.t_in_b,
                pair.plan.t_end_b,
                pair.plan.incoming_offset_end
            );
        }
        entry = mixless_mixplan::established_entry(&pair.plan, &b.analysis);
        earliest = pair.plan.t_end_b;
        offset_a = pair.plan.incoming_offset_end;
        pairs[i] = Some(pair);
    }
    Some(pairs)
}

#[cfg(test)]
#[path = "preparation/tests.rs"]
mod tests;

#[cfg(test)]
pub(crate) fn cache_best_order(core: &AppCore, tracks: &[TrackId], order: &[usize]) {
    core.library
        .save_best_order(&best_order_key(core, tracks), order)
        .unwrap();
}

#[cfg(test)]
pub(crate) fn best_order_key(core: &AppCore, tracks: &[TrackId]) -> String {
    let prepared: Vec<_> = tracks
        .iter()
        .map(|id| crate::analysis::prepare(core, *id).unwrap())
        .collect();
    let options = mixless_mixplan::PlannerOptions {
        harmonic_key_shift: true,
        live_moves: core.settings.get().live_moves,
        ..Default::default()
    };
    order_cache::key(&prepared, &options).unwrap()
}
