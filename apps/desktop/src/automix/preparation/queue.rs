//! Playback-owned order publication. A request is not an applied queue.
use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OrderMode {
    #[default]
    Order,
    Shuffle,
    Best,
}

impl OrderMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Order => "ORDER",
            Self::Shuffle => "SHUFFLE",
            Self::Best => "BEST",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::Order => Self::Shuffle,
            Self::Shuffle => Self::Best,
            Self::Best => Self::Order,
        }
    }
}

/// An occurrence within the source, not merely a recording ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct QueueEntry {
    pub track: TrackId,
    pub occurrence: usize,
}

pub fn entries(tracks: &[TrackId]) -> Vec<QueueEntry> {
    let mut counts = HashMap::new();
    tracks
        .iter()
        .map(|id| {
            let occurrence = counts.entry(*id).or_insert(0);
            let entry = QueueEntry {
                track: *id,
                occurrence: *occurrence,
            };
            *occurrence += 1;
            entry
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::automix) struct Intent {
    source: u64,
    pub revision: u64,
    pub mode: OrderMode,
}

#[derive(Default)]
pub(super) struct QueueState {
    pub requested: OrderMode,
    pub target: Intent,
    pub prepared_mode: OrderMode,
    pub applied: Option<Intent>,
    pub rows: Arc<Vec<QueueEntry>>,
    pub pair: Option<[QueueEntry; 2]>,
    pub outgoing: Option<mixless_protocol::DeckId>,
    pub active: Option<Arc<mixless_protocol::MixPlan>>,
    pub plans: Vec<Option<Arc<mixless_protocol::MixPlan>>>,
    pub error: Option<String>,
}

impl QueueState {
    pub fn reset(&mut self, mode: OrderMode) {
        self.target.source += 1;
        self.requested = mode;
        self.target.mode = mode;
        self.prepared_mode = mode;
        self.applied = None;
        self.rows = Arc::default();
        self.pair = None;
        self.outgoing = None;
        self.active = None;
        self.plans.clear();
        self.error = None;
    }
}

#[derive(Clone, Default)]
pub struct QueueView {
    pub version: u64,
    pub requested: OrderMode,
    pub effective: OrderMode,
    pub pending: bool,
    pub rows: Arc<Vec<QueueEntry>>,
    pub pair: Option<[QueueEntry; 2]>,
    pub outgoing: Option<mixless_protocol::DeckId>,
    pub preview: preview::Preview,
    pub error: Option<String>,
}

impl Preparation {
    pub fn queue_best_ready(&self) -> bool {
        self.sequence.lock().expect("set plan").queue.target.mode == OrderMode::Best
    }
    pub fn queue_view(&self) -> QueueView {
        let sequence = self.sequence.lock().expect("set plan");
        let q = &sequence.queue;
        let effective = q.applied.map_or(OrderMode::Order, |i| i.mode);
        QueueView {
            version: self.preview_revision.load(Ordering::Acquire),
            requested: q.requested,
            effective,
            pending: q.requested != effective || q.applied.is_some_and(|i| i != q.target),
            rows: q.rows.clone(),
            pair: q.pair,
            outgoing: q.outgoing,
            preview: preview::Preview {
                plans: q.plans.clone(),
                active: q.active.clone(),
            },
            error: q.error.clone(),
        }
    }

    pub(in crate::automix) fn queue_intent(&self) -> Intent {
        let sequence = self.sequence.lock().expect("set plan");
        Intent {
            mode: sequence.queue.prepared_mode,
            ..sequence.queue.target
        }
    }

    pub(in crate::automix) fn queue_input(&self) -> (Vec<TrackId>, Intent) {
        // Same lock order as prepare: identity, mode and rows are one request.
        let tracks = self.playlist.lock().expect("playlist preparation");
        let sequence = self.sequence.lock().expect("set plan");
        (
            tracks.clone(),
            Intent {
                mode: sequence.queue.prepared_mode,
                ..sequence.queue.target
            },
        )
    }

    pub(super) fn request_mode(&self, mode: OrderMode) {
        let mut sequence = self.sequence.lock().expect("set plan");
        sequence.queue.requested = mode;
        sequence.queue.error = None;
        if mode != OrderMode::Best {
            sequence.queue.target.mode = mode;
        }
        self.preview_revision.fetch_add(1, Ordering::Release);
    }

    pub(super) fn best_ready(&self) {
        let mut sequence = self.sequence.lock().expect("set plan");
        sequence.queue.requested = OrderMode::Best;
        sequence.queue.target.mode = OrderMode::Best;
    }

    /// Only a worker which actually staged this revision can acknowledge it.
    #[cfg(test)]
    pub(in crate::automix) fn publish_queue(&self, intent: Intent, rows: Vec<QueueEntry>) -> bool {
        self.publish_queue_on(intent, rows, mixless_protocol::DeckId::A)
    }

    pub(in crate::automix) fn publish_queue_on(
        &self,
        intent: Intent,
        rows: Vec<QueueEntry>,
        outgoing: mixless_protocol::DeckId,
    ) -> bool {
        let mut sequence = self.sequence.lock().expect("set plan");
        if sequence.queue.target != intent {
            return false;
        }
        let pair = rows
            .first()
            .copied()
            .zip(rows.get(1).or_else(|| rows.first()).copied())
            .map(|(a, b)| [a, b]);
        // Advancing the playhead within the same circuit moves the markers,
        // not the whole table. A new shuffle bag or intent publishes new rows.
        let old = &sequence.queue.rows;
        let same_cycle = old.len() == rows.len()
            && (0..rows.len()).any(|offset| {
                rows.iter()
                    .enumerate()
                    .all(|(i, row)| *row == old[(i + offset) % old.len()])
            });
        let changed = sequence.queue.applied != Some(intent) || !same_cycle;
        let active = sequence.anchor.clone().filter(|p| {
            p.summary.as_ref().map(|s| s.pair) == pair.map(|p| (p[0].track, p[1].track))
        });
        let active_changed = sequence.queue.pair != pair
            || sequence.queue.outgoing != Some(outgoing)
            || sequence.queue.active.as_ref().map(Arc::as_ptr) != active.as_ref().map(Arc::as_ptr);
        sequence.queue.pair = pair;
        sequence.queue.outgoing = Some(outgoing);
        sequence.queue.active = active;
        sequence.anchor_entries = sequence.queue.active.as_ref().and(pair);
        sequence.remember_active(&rows);
        sequence.queue.applied = Some(intent);
        if changed {
            sequence.queue.rows = Arc::new(rows);
        }
        sequence.queue.plans = sequence.aligned_previews(&sequence.queue.rows);
        if changed || active_changed {
            self.preview_revision.fetch_add(1, Ordering::Release);
        }
        true
    }

    pub fn end_queue_session(&self) {
        let mut sequence = self.sequence.lock().expect("set plan");
        sequence.queue.applied = None;
        sequence.queue.rows = Arc::default();
        sequence.queue.pair = None;
        sequence.queue.outgoing = None;
        sequence.queue.active = None;
        sequence.queue.plans.clear();
        // A completed cache can still be reused, but cannot select a stopped session.
        self.defaulting.store(false, Ordering::Release);
        self.best.store(false, Ordering::Release);
        sequence.queue.requested = OrderMode::Order;
        sequence.queue.target.mode = OrderMode::Order;
        sequence.queue.target.source += 1;
        self.preview_revision.fetch_add(1, Ordering::Release);
    }
}
