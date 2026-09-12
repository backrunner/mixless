//! AutoMix session intent, worker lifecycle and UI progress.
use super::*;

impl UiState {
    pub fn toggle_automix(&mut self) {
        self.sync_request = None;
        if self.automix_active {
            self.stop_automix();
            return;
        }
        let tracks: Vec<_> = self.tracks.iter().map(|track| track.id).collect();
        if tracks.is_empty() {
            self.error = "Import a track to start Automix".into();
            return;
        }
        for epoch in self.deck_load_epoch.iter() {
            epoch.fetch_add(1, Ordering::AcqRel);
        }
        self.deck_load_rx = [None, None];
        self.grid_rx = [None, None];
        self.start_automix_worker(tracks);
    }

    fn start_automix_worker(&mut self, tracks: Vec<TrackId>) {
        self.automix_retry = None;
        let generation = self.automix_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.automix_active = true;
        self.automix_status = "Loading next track".into();
        self.error = "".into();
        let (tx, rx) = channel();
        self.automix_rx = Some(rx);
        let core = self.core.clone();
        let epoch = self.automix_epoch.clone();
        let shuffle = self.automix_shuffle.clone();
        std::thread::spawn(move || {
            crate::automix::run(core, tracks, shuffle, epoch, generation, tx)
        });
    }

    pub fn stop_automix(&mut self) {
        let core = self.core.clone();
        let _commit = core.automix_commit.lock().expect("automix commit");
        self.automix_epoch.fetch_add(1, Ordering::AcqRel);
        self.automix_active = false;
        self.automix_retry = None;
        self.automix_status.clear();
        self.automix_plan = None;
        self.automix_rx = None;
        self.dispatch(Command::StopAutomix);
    }

    pub(super) fn poll_automix(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = self.automix_rx.take() {
            let mut finished = false;
            while let Ok(message) = rx.try_recv() {
                changed = true;
                match message {
                    crate::automix::AutomixMsg::Status(status) => self.automix_status = status,
                    crate::automix::AutomixMsg::Preparing(title) => {
                        self.automix_plan = None;
                        self.automix_status = format!("Loading {title}");
                    }
                    crate::automix::AutomixMsg::Plan(deck, plan) => {
                        self.automix_plan = Some((deck, plan))
                    }
                    crate::automix::AutomixMsg::Done(result) => {
                        finished = true;
                        self.automix_plan = None;
                        self.automix_status = match result {
                            Ok(()) => "Waiting for playback".into(),
                            Err(error) => {
                                tracing::debug!("Automix waiting: {error}");
                                "Waiting for a playable transition".into()
                            }
                        };
                        self.automix_retry =
                            Some(std::time::Instant::now() + std::time::Duration::from_secs(2));
                    }
                }
            }
            if !finished {
                self.automix_rx = Some(rx);
            }
        }
        if self.automix_active
            && self
                .automix_retry
                .is_some_and(|time| std::time::Instant::now() >= time)
            && self.deck_load_rx.iter().all(Option::is_none)
            && !self.tracks.is_empty()
        {
            self.start_automix_worker(self.tracks.iter().map(|t| t.id).collect());
            changed = true;
        }
        changed
    }
}
