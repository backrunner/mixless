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
    pub pairs: Vec<Option<ScheduledPair>>,
    // The engine owns this decision. New evidence may only change its suffix.
    pub anchor: Option<Arc<MixPlan>>,
    pub following: Option<Vec<TrackId>>,
    displayed: Vec<Option<Arc<MixPlan>>>,
}

impl Sequence {
    pub fn select(&mut self, tracks: &[TrackId]) {
        self.following = None;
        self.pairs = vec![None; tracks.len()];
        // Keep unchanged neighbours through a reorder. A preview belongs to
        // track IDs, never to the old row indices or another playlist's edges.
        self.displayed.retain(|plan| {
            adjacent_index(
                tracks,
                plan.as_ref()
                    .and_then(|p| p.summary.as_ref().map(|s| s.pair)),
            )
            .is_some()
        });
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
            self.displayed.retain(|old| {
                old.as_ref()
                    .and_then(|p| p.summary.as_ref())
                    .is_some_and(|s| s.pair == pair || (s.pair.0 != pair.0 && s.pair.1 != pair.1))
            });
        }
        // Show this committed pair immediately, together with the previous
        // forecast except edges it superseded. The overlay also rejects
        // incompatible IN/OUT ranges on a shared track.
        self.anchor = Some(plan);
    }
}

impl Preparation {
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
        sequence.displayed = pairs
            .iter()
            .map(|pair| pair.as_ref().map(|p| p.plan.clone()))
            .collect();
        sequence.pairs = pairs;
        self.preview_revision.fetch_add(1, Ordering::Release);
        true
    }
}

#[cfg(test)]
mod tests;
