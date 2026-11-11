//! Keep the displayed forecast while its execution cache is being rebuilt.
use super::*;
use mixless_protocol::MixPlan;

#[derive(Clone, Default)]
pub struct Preview {
    pub plans: Vec<Option<Arc<MixPlan>>>,
    pub active: Option<Arc<MixPlan>>,
}

#[derive(Default)]
pub(super) struct Sequence {
    pub queue: super::queue::QueueState,
    pub pairs: Vec<Option<ScheduledPair>>,
    // The engine owns this decision. New evidence may only change its suffix.
    pub anchor: Option<Arc<MixPlan>>,
    pub anchor_entries: Option<[QueueEntry; 2]>,
    pub following: Option<Vec<QueueEntry>>,
    pub(super) displayed: Vec<Option<Arc<MixPlan>>>,
    source_entries: Vec<QueueEntry>,
    displayed_entries: Vec<QueueEntry>,
}

impl Sequence {
    pub fn remember_active(&mut self, rows: &[QueueEntry]) {
        let Some(pair) = self.anchor_entries else {
            return;
        };
        let Some(plan) = self.anchor.clone() else {
            return;
        };
        // A commit may precede the first background forecast. Seed history
        // from the acknowledged occurrences so that its entrance survives
        // the next handoff even when no forecast has arrived yet.
        self.displayed = self.aligned_previews(rows);
        self.displayed_entries = rows.to_vec();
        if let Some(i) = (0..self.displayed_entries.len()).find(|i| {
            [
                self.displayed_entries[*i],
                self.displayed_entries[(*i + 1) % self.displayed_entries.len()],
            ] == pair
        }) {
            self.displayed[i] = Some(plan);
        }
    }

    pub fn select(&mut self, tracks: &[TrackId]) {
        self.source_entries = entries(tracks);
        self.following = None;
        self.pairs = vec![None; tracks.len()];
        // Keep slots attached to their exact occurrences until the new
        // forecast is ready. The applied queue filters unchanged neighbours.
        if tracks.is_empty() {
            self.displayed.clear();
            self.displayed_entries.clear();
        } else {
            for (i, old) in self.displayed.iter_mut().enumerate() {
                let pair = self.displayed_entries.get(i).copied().zip(
                    self.displayed_entries
                        .get((i + 1) % self.displayed_entries.len().max(1))
                        .copied(),
                );
                if !pair.is_some_and(|(a, b)| {
                    (0..self.source_entries.len()).any(|j| {
                        (
                            self.source_entries[j],
                            self.source_entries[(j + 1) % self.source_entries.len()],
                        ) == (a, b)
                    })
                }) {
                    *old = None;
                }
            }
        }
    }

    pub(super) fn aligned_previews(&self, rows: &[QueueEntry]) -> Vec<Option<Arc<MixPlan>>> {
        (0..rows.len())
            .map(|i| {
                let pair = (rows[i], rows[(i + 1) % rows.len()]);
                self.displayed.iter().enumerate().find_map(|(j, plan)| {
                    let plan = plan.as_ref()?;
                    let a = *self.displayed_entries.get(j)?;
                    let b = *self
                        .displayed_entries
                        .get((j + 1) % self.displayed_entries.len())?;
                    (pair == (a, b) && plan.summary.as_ref()?.pair == (a.track, b.track))
                        .then(|| plan.clone())
                })
            })
            .collect()
    }

    pub fn commit(&mut self, tracks: &[TrackId], plan: Arc<MixPlan>) {
        let pair = plan.summary.as_ref().map(|s| s.pair);
        let index = adjacent_index(tracks, pair);
        if let Some(i) = index {
            let unchanged = self
                .pairs
                .get(i)
                .and_then(Option::as_ref)
                .is_some_and(|old| Arc::ptr_eq(&old.plan, &plan));
            if !unchanged {
                // Retained display data must never become an executable old
                // exit after the live entrance has changed.
                self.pairs = vec![None; tracks.len()];
            }
            self.pairs.resize(tracks.len(), None);
            let key = self.pairs[i]
                .as_ref()
                .map_or(String::new(), |p| p.key.clone());
            self.pairs[i] = Some(ScheduledPair {
                key,
                plan: plan.clone(),
            });
        } else {
            self.pairs = vec![None; tracks.len()];
        }
        if let Some(pair) = pair {
            for old in &mut self.displayed {
                if !old
                    .as_ref()
                    .and_then(|p| p.summary.as_ref())
                    .is_some_and(|s| s.pair == pair || (s.pair.0 != pair.0 && s.pair.1 != pair.1))
                {
                    *old = None;
                }
            }
        }
        // Show this committed pair immediately, together with the previous
        // forecast except edges it superseded. The overlay also rejects
        // incompatible IN/OUT ranges on a shared track.
        // The worker publishes the exact occurrences with the queue. Recording
        // IDs alone cannot move a repeated track's plan to the correct row.
        self.anchor_entries = None;
        self.anchor = Some(plan);
    }
}

impl Preparation {
    #[cfg(test)]
    pub fn previews(&self) -> Preview {
        let sequence = self.sequence.lock().expect("set plan");
        Preview {
            plans: sequence.displayed.clone(),
            active: sequence.anchor.clone(),
        }
    }

    pub(super) fn publish_previews(
        &self,
        revision: u64,
        pairs: Vec<Option<ScheduledPair>>,
    ) -> bool {
        let mut sequence = self.sequence.lock().expect("set plan");
        if self.revision.load(Ordering::Acquire) != revision {
            return false;
        }
        let rows = sequence
            .following
            .clone()
            .unwrap_or_else(|| sequence.source_entries.clone());
        let previous = sequence.aligned_previews(&rows);
        let completed_into = sequence.anchor_entries.map(|p| p[0]);
        sequence.displayed = pairs
            .iter()
            .enumerate()
            .map(|(i, pair)| {
                pair.as_ref().map(|p| p.plan.clone()).or_else(|| {
                    // This edge is deliberately outside the next-pass forecast:
                    // it is how the currently sounding occurrence entered. Keep
                    // its completed IN/OUT, without making it executable again.
                    (rows.get((i + 1) % rows.len().max(1)).copied() == completed_into
                        && completed_into.is_some())
                    .then(|| previous.get(i).cloned().flatten())
                    .flatten()
                })
            })
            .collect();
        sequence.displayed_entries = rows;
        sequence.pairs = pairs;
        if sequence.queue.applied == Some(sequence.queue.target) {
            sequence.queue.plans = sequence.aligned_previews(&sequence.queue.rows);
        }
        self.preview_revision.fetch_add(1, Ordering::Release);
        true
    }
}

#[cfg(test)]
mod tests;
