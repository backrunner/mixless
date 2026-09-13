//! Host publication only; the callback retains the last immutable source on contention.
use super::*;

pub(super) struct PreparedStems {
    pub source: Arc<AudioBuffer>,
    pub audio: Arc<crate::StemBuffer>,
}
impl Engine {
    pub fn stem_source(&self, deck: DeckId, id: TrackId) -> Option<Arc<AudioBuffer>> {
        let slot = &self.shared.decks[deck.index()];
        let buffer = slot.buffer.lock().ok()?;
        (slot.track_id.load(Ordering::Acquire) == id.0 as u64)
            .then(|| buffer.clone())
            .flatten()
    }
    /// The source identity, dimensions and finite samples are checked off callback.
    pub fn attach_stems(
        &self,
        deck: DeckId,
        id: TrackId,
        source: &Arc<AudioBuffer>,
        audio: Arc<crate::StemBuffer>,
    ) -> Result<bool, EngineError> {
        if source.frames != audio.frames || source.sample_rate != audio.sample_rate {
            return Err(EngineError::Protocol("Stem source alignment mismatch"));
        }
        let slot = &self.shared.decks[deck.index()];
        let buffer = slot
            .buffer
            .lock()
            .map_err(|_| EngineError::Protocol("source lock poisoned"))?;
        if slot.track_id.load(Ordering::Acquire) != id.0 as u64
            || buffer.as_ref().is_none_or(|b| !Arc::ptr_eq(b, source))
        {
            return Ok(false);
        }
        let mut current = slot
            .stems
            .lock()
            .map_err(|_| EngineError::Protocol("stem publication lock poisoned"))?;
        if current
            .as_ref()
            .is_some_and(|s| Arc::ptr_eq(&s.source, source))
        {
            return Ok(true);
        }
        let next = Arc::new(PreparedStems {
            source: source.clone(),
            audio,
        });
        self.retire_stems(current.replace(next));
        Ok(true)
    }
    pub fn stems_ready(&self, deck: DeckId) -> bool {
        self.shared.decks[deck.index()].stems_ready()
    }
    fn retire_stems(&self, old: Option<Arc<PreparedStems>>) {
        let mut retired = self.retired_stems.lock().expect("retired stems");
        retired.retain(|s| Arc::strong_count(s) > 1);
        if let Some(old) = old {
            retired.push(old);
        }
    }
    pub(super) fn clear_stems(&self, deck: DeckId) {
        let slot = &self.shared.decks[deck.index()];
        self.retire_stems(slot.stems.lock().expect("stems").take());
        for gain in &slot.stem_gain {
            gain.store(1000, Ordering::Relaxed);
        }
    }
}
impl DeckSlot {
    pub(super) fn stems_ready(&self) -> bool {
        let Ok(buffer) = self.buffer.try_lock() else {
            return false;
        };
        let Ok(stems) = self.stems.try_lock() else {
            return false;
        };
        stems
            .as_ref()
            .zip(buffer.as_ref())
            .is_some_and(|(s, b)| Arc::ptr_eq(&s.source, b))
    }
}
