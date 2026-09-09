//! An explicit Sync request may wait for offline analysis, but never outlive
//! its tracks or a manual takeover. Audio timing remains entirely in the engine.
use mixless_protocol::{Command, DeckId, EngineSnapshot, TrackId};

pub struct SyncRequest {
    pub follower: DeckId,
    tracks: [TrackId; 2],
}

pub enum Progress {
    Waiting,
    Ready(Command),
    Cancelled,
    Unavailable,
}

impl SyncRequest {
    pub fn new(follower: DeckId, snapshot: &EngineSnapshot) -> Option<Self> {
        if snapshot.decks.iter().any(|d| d.frames == 0 || d.reverse) {
            return None;
        }
        Some(Self {
            follower,
            tracks: [snapshot.decks[0].track_id?, snapshot.decks[1].track_id?],
        })
    }

    pub fn progress(&self, snapshot: &EngineSnapshot, analyzing: [bool; 2]) -> Progress {
        if snapshot.automix_on
            || snapshot
                .decks
                .iter()
                .enumerate()
                .any(|(i, d)| d.track_id != Some(self.tracks[i]) || d.frames == 0 || d.reverse)
        {
            return Progress::Cancelled;
        }
        if snapshot.decks.iter().all(|d| d.grid_ready) {
            return Progress::Ready(Command::Sync {
                deck: self.follower,
                keylock: snapshot.deck(self.follower).keylock,
            });
        }
        if snapshot
            .decks
            .iter()
            .enumerate()
            .any(|(i, d)| !d.grid_ready && !analyzing[i])
        {
            return Progress::Unavailable;
        }
        Progress::Waiting
    }

    pub fn cancelled_by(&self, command: &Command) -> bool {
        match command {
            Command::LoadDeck { .. }
            | Command::Eject { .. }
            | Command::SetReverse { .. }
            | Command::SetJogTouch { touching: true, .. }
            | Command::Jog { .. }
            | Command::StartAutomix { .. } => true,
            Command::SetRate { deck, .. }
            | Command::DisableSync { deck }
            | Command::Sync { deck, .. } => *deck == self.follower,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn loaded() -> EngineSnapshot {
        let mut s = EngineSnapshot::default();
        for (i, d) in s.decks.iter_mut().enumerate() {
            d.track_id = Some(TrackId(i as i64 + 1));
            d.frames = 48_000 * 60;
        }
        s
    }
    #[test]
    fn waits_for_both_grids_then_syncs_once_without_starting_transport() {
        let mut s = loaded();
        let request = SyncRequest::new(DeckId::B, &s).unwrap();
        assert!(matches!(request.progress(&s, [true; 2]), Progress::Waiting));
        s.decks[0].grid_ready = true;
        assert!(matches!(
            request.progress(&s, [false, true]),
            Progress::Waiting
        ));
        s.decks[1].grid_ready = true;
        s.decks[1].keylock = false;
        assert!(matches!(
            request.progress(&s, [false; 2]),
            Progress::Ready(Command::Sync {
                deck: DeckId::B,
                keylock: false
            })
        ));
    }
    #[test]
    fn failed_analysis_and_track_changes_do_not_trigger_later_sync() {
        let mut s = loaded();
        let request = SyncRequest::new(DeckId::B, &s).unwrap();
        assert!(matches!(
            request.progress(&s, [false, true]),
            Progress::Unavailable
        ));
        s.decks[0].track_id = Some(TrackId(3));
        s.decks.iter_mut().for_each(|d| d.grid_ready = true);
        assert!(matches!(
            request.progress(&s, [false; 2]),
            Progress::Cancelled
        ));
    }
    #[test]
    fn manual_takeover_cancels_but_master_tempo_and_play_do_not() {
        let request = SyncRequest::new(DeckId::B, &loaded()).unwrap();
        assert!(request.cancelled_by(&Command::SetJogTouch {
            deck: DeckId::A,
            touching: true
        }));
        assert!(request.cancelled_by(&Command::SetRate {
            deck: DeckId::B,
            rate: 1.1
        }));
        assert!(!request.cancelled_by(&Command::SetRate {
            deck: DeckId::A,
            rate: 1.1
        }));
        assert!(!request.cancelled_by(&Command::PlayPause { deck: DeckId::B }));
        assert!(SyncRequest::new(DeckId::A, &EngineSnapshot::default()).is_none());
    }
}
