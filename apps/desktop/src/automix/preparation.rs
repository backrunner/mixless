//! Prepare one continuous set and publish its dependent decisions together.
use super::*;
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

#[derive(Default)]
struct Sequence {
    pairs: Vec<Option<ScheduledPair>>,
    // The engine owns this decision. New evidence may only change its suffix.
    anchor: Option<Arc<mixless_protocol::MixPlan>>,
    following: Option<Vec<TrackId>>,
}

#[derive(Default)]
pub struct Preparation {
    playlist: Mutex<Vec<TrackId>>,
    revision: AtomicU64,
    input_revision: AtomicU64,
    sender: OnceLock<Sender<(u64, Vec<TrackId>)>>,
    pub(super) plans: Mutex<HashMap<String, mixless_protocol::MixPlan>>,
    pub(super) order: Mutex<VecDeque<String>>,
    sequence: Mutex<Sequence>,
    pub preview_revision: AtomicU64,
}

impl Preparation {
    pub fn previews(&self) -> Vec<Option<Arc<mixless_protocol::MixPlan>>> {
        self.sequence
            .lock()
            .expect("set plan")
            .pairs
            .iter()
            .map(|pair| pair.as_ref().map(|p| p.plan.clone()))
            .collect()
    }
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
    pub(super) fn tracks(&self) -> Vec<TrackId> {
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
    let mut queue = vec![incoming];
    queue.extend(order.preview(count, shuffle));
    if queue.len() > 1 && queue.last() == Some(&incoming) {
        queue.pop();
    }
    let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
    if sequence.following.as_ref() == Some(&queue) {
        return;
    }
    sequence.following = Some(queue);
    drop(sequence);
    prepare(core, core.mix_preparation.tracks(), true, false);
}

pub fn prepare_playlist(core: &Arc<AppCore>, tracks: Vec<TrackId>) {
    prepare(core, tracks, false, true);
}

pub fn refresh_previews(core: &Arc<AppCore>) {
    prepare(core, core.mix_preparation.tracks(), true, true);
}

pub(super) fn reset_session(core: &Arc<AppCore>) {
    let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
    sequence.following = None;
    let changed = sequence.anchor.take().is_some();
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
    let pair = plan.summary.as_ref().map(|s| s.pair);
    let index = adjacent_index(&tracks, pair);
    // Reuse the still-valid forecast, but never combine a changed entrance
    // with an old exit. The worker replaces the entire suffix atomically.
    if let Some(i) = index {
        let unchanged = sequence
            .pairs
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|old| Arc::ptr_eq(&old.plan, &plan));
        if !unchanged {
            sequence.pairs = vec![None; tracks.len()];
        }
        if sequence.pairs.len() != tracks.len() {
            sequence.pairs.resize(tracks.len(), None);
        }
        let key = sequence.pairs[i]
            .as_ref()
            .map_or(String::new(), |p| p.key.clone());
        sequence.pairs[i] = Some(ScheduledPair {
            key,
            plan: plan.clone(),
        });
    } else {
        // Shuffle has no honest ordered-set forecast. Its committed pair is
        // shown by the active overlay until a concrete order is available.
        sequence.pairs = vec![None; tracks.len()];
    }
    sequence.anchor = Some(plan);
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
    if !changed && !force {
        return;
    }
    if input {
        core.mix_preparation
            .input_revision
            .fetch_add(1, Ordering::AcqRel);
    }
    *current = tracks.clone();
    let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
    let revision = core.mix_preparation.revision.fetch_add(1, Ordering::AcqRel) + 1;
    if changed {
        sequence.following = None;
        sequence.pairs = vec![None; tracks.len()];
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
                let mut sequence = core.mix_preparation.sequence.lock().expect("set plan");
                if !active() {
                    continue;
                }
                sequence.pairs = pairs;
                core.mix_preparation
                    .preview_revision
                    .fetch_add(1, Ordering::Release);
                drop(sequence);
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
        processor
            .cached(&track.track.content_hash, track.analysis.duration_sec)
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
    let following = core
        .mix_preparation
        .sequence
        .lock()
        .expect("set plan")
        .following
        .clone();
    let tracks = following.as_deref().unwrap_or(tracks);
    let mut pairs = vec![None; tracks.len()];
    if tracks.len() < 2 {
        return Some(pairs);
    }
    let anchor = core
        .mix_preparation
        .sequence
        .lock()
        .expect("set plan")
        .anchor
        .clone();
    let anchor_index = adjacent_index(
        tracks,
        anchor
            .as_ref()
            .and_then(|p| p.summary.as_ref().map(|s| s.pair)),
    );
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
    let root = anchor
        .as_ref()
        .and_then(|p| p.summary.as_ref())
        .map(|s| s.pair.0);
    let (first, count, mut entry, mut earliest, mut offset_a) = if let Some(plan) = anchor {
        let pair = plan.summary.as_ref()?.pair;
        let Some(incoming) = tracks.iter().position(|id| *id == pair.1) else {
            return Some(pairs);
        };
        let cursor = (plan.t_in_b, plan.t_end_b, plan.incoming_offset_end);
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
        if Some(tracks[j]) == root {
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
        entry = pair.plan.t_in_b;
        earliest = pair.plan.t_end_b;
        offset_a = pair.plan.incoming_offset_end;
        pairs[i] = Some(pair);
    }
    Some(pairs)
}

#[cfg(test)]
#[path = "preparation/tests.rs"]
mod tests;
