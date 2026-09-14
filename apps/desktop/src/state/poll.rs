//! Apply asynchronous results and refresh UI state.
use super::*;

impl UiState {
    pub fn poll(&mut self) -> bool {
        let mut changed = self.poll_transport_press();
        let preview_revision = self
            .core
            .mix_preparation
            .preview_revision
            .load(Ordering::Acquire);
        if preview_revision != self.automix_preview_revision {
            self.automix_preview_revision = preview_revision;
            changed |= self.automix_active;
        }
        changed |= self.poll_audio();
        changed |= self.poll_library_order();
        changed |= self.poll_library();
        changed |= self.poll_library_actions();
        if self.library_previews.poll() {
            self.library_key = None;
            changed = true;
        }
        let analysis_revision = self.core.analysis.revision.load(Ordering::Acquire);
        if self.analysis_revision != analysis_revision {
            self.analysis_revision = analysis_revision;
            let updates = self.core.analysis.take_updates();
            for id in updates.keys() {
                self.library_previews.invalidate(*id);
            }
            if self
                .tracks
                .iter()
                .any(|track| updates.contains_key(&track.id))
            {
                for track in Arc::make_mut(&mut self.tracks) {
                    if let Some(updated) = updates.get(&track.id) {
                        *track = updated.clone();
                    }
                }
            }
            if !self.busy {
                self.acquire = self.core.analysis.summary().into();
            }
            changed = true;
        }
        let revision = self.core.settings.revision();
        if self.settings_revision != revision {
            let settings = self.core.settings.get();
            self.wave_layout = settings.wave_layout;
            self.show_fx = settings.show_fx;
            self.settings_revision = revision;
            changed = true;
        }
        changed |= self.poll_automix();
        let midi_commands = self
            .core
            .midi
            .lock()
            .ok()
            .and_then(|midi| midi.as_ref().map(|hub| hub.drain()))
            .unwrap_or_default();
        for cmd in midi_commands {
            match cmd {
                Command::Sync { deck, .. } => self.sync(deck),
                Command::JumpCue { deck, index } => self.trigger_cue(deck, index as usize, false),
                cmd => self.dispatch(cmd),
            }
        }
        let snapshot = self.core.engine.snapshot();
        if self.automix_active {
            if let Some(deck) = super::automix::newly_playing_deck(&self.snapshot, &snapshot) {
                self.focus = deck;
                changed = true;
            }
        }
        changed |= snapshot != self.snapshot;
        self.fx = std::array::from_fn(|index| snapshot.decks[index].fx);
        self.snapshot = snapshot;
        self.presentation_frames = self.core.engine.presentation_frames(&self.snapshot);

        for index in 0..2 {
            if let Some(rx) = self.deck_load_rx[index].take() {
                match rx.try_recv() {
                    Ok(Ok(())) => {
                        self.deck_loading[index] = None;
                        if std::mem::take(&mut self.pending_play[index]) {
                            self.dispatch(Command::PlayPause {
                                deck: DeckId::from_index(index).unwrap(),
                            });
                        }
                        changed = true;
                    }
                    Ok(Err(error)) => {
                        self.deck_loading[index] = None;
                        self.pending_play[index] = false;
                        self.grid_rx[index] = None;
                        self.error = error.into();
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        self.deck_load_rx[index] = Some(rx)
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.deck_loading[index] = None;
                        self.pending_play[index] = false;
                        self.grid_rx[index] = None;
                        self.error = "Deck load worker stopped unexpectedly".into();
                        changed = true;
                    }
                }
            }
        }
        for wave in self.wave_cache.iter().flatten() {
            changed |= wave.poll_tiles();
        }
        for (i, deck_id) in [DeckId::A, DeckId::B].into_iter().enumerate() {
            let tid = self.snapshot.decks[i].track_id.map(|t| t.0);
            let cached = self.wave[i].as_ref().map(|(id, _)| *id);
            let current_wave = self.core.engine.deck_waveform(deck_id);
            let wave_changed = match (&self.wave[i], &current_wave) {
                (Some((_, old)), Some(current)) => !Arc::ptr_eq(old, current),
                (None, None) => false,
                _ => true,
            };
            changed |= self.deck_artwork[i].poll(
                &self.core,
                &self.artwork_cache,
                self.snapshot.decks[i].track_id,
                wave_changed,
            );
            if tid != cached || wave_changed {
                changed = true;
                self.wave_tempo[i] = None;
                self.wave[i] = current_wave.map(|w| (tid.unwrap_or(-1), w));
                // Same track, upgraded waveform (envelope → analyzed): keep the
                // old raster until the rebuilt cache arrives on wave_cache_rx.
                if tid != cached {
                    self.wave_cache[i] = None;
                }
                self.wave_cache_rx[i] = None;
                if let Some((_, wave)) = &self.wave[i] {
                    let (tx, rx) = channel();
                    let wave = wave.clone();
                    self.wave_cache_rx[i] = Some(rx);
                    std::thread::spawn(move || {
                        let _ = tx.send(Arc::new(crate::wave::WaveCache::new(wave)));
                    });
                }
            }
            if let Some(rx) = self.wave_cache_rx[i].take() {
                match rx.try_recv() {
                    Ok(cache) => {
                        if self.wave[i]
                            .as_ref()
                            .is_some_and(|(_, wave)| Arc::ptr_eq(wave, &cache.source))
                        {
                            self.wave_cache[i] = Some(cache);
                            changed = true;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => self.wave_cache_rx[i] = Some(rx),
                    Err(_) => {}
                }
            }
            if tid.is_some() && self.wave_tempo[i].is_none() {
                if let Ok(loaded) = self.core.analysis.loaded.try_lock() {
                    if let Some(prepared) = &loaded[i] {
                        if Some(prepared.track.id.0) == tid {
                            self.wave_tempo[i] = Some(Arc::new(prepared.analysis.tempo.clone()));
                            changed = true;
                        }
                    }
                }
            }
        }

        for i in 0..2 {
            if let Some(rx) = self.grid_rx[i].take() {
                match rx.try_recv() {
                    Ok(result) => {
                        match result {
                            Ok(tempo) => self.wave_tempo[i] = Some(Arc::new(tempo)),
                            Err(error) => {
                                self.error =
                                    format!("Deck {}: {error}", if i == 0 { "A" } else { "B" })
                                        .into()
                            }
                        }
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => self.grid_rx[i] = Some(rx),
                    Err(_) => {}
                }
            }
        }
        if let Some(request) = self.sync_request.take() {
            use crate::beat_sync::Progress;
            let snapshot = self.core.engine.snapshot();
            match request.progress(
                &snapshot,
                std::array::from_fn(|i| self.grid_rx[i].is_some()),
            ) {
                Progress::Waiting => self.sync_request = Some(request),
                Progress::Ready(command) => {
                    self.dispatch(command);
                    changed = true;
                }
                Progress::Cancelled => changed = true,
                Progress::Unavailable => {
                    if self.error.is_empty() {
                        self.error =
                            "Beat Sync unavailable: no usable beat grid on one or both tracks"
                                .into();
                    }
                    changed = true;
                }
            }
        }
        if let Some(rx) = self.import_rx.take() {
            let mut finished = false;
            loop {
                match rx.try_recv() {
                    Ok(ImportMsg::LibraryChanged) => {
                        self.refresh_tracks();
                        changed = true;
                    }
                    Ok(ImportMsg::Progress(p)) => {
                        self.acquire = p.into();
                        changed = true;
                    }
                    Ok(ImportMsg::Done(Ok(report))) => {
                        self.acquire = report.summary().into();
                        self.import_details =
                            report.warning.into_iter().chain(report.errors).collect();

                        self.busy = false;
                        finished = true;
                        changed = true;
                    }
                    Ok(ImportMsg::Done(Err(e))) => {
                        self.error = e.into();
                        self.busy = false;
                        finished = true;
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if !finished {
                            self.error = "Import stopped before all files were added.".into();
                            self.busy = false;
                            finished = true;
                            changed = true;
                        }
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                }
            }
            if finished {
                self.refresh_tracks();
                self.start_next_import();
            } else {
                self.import_rx = Some(rx);
            }
        }
        if let Some(rx) = self.artwork_rx.take() {
            match rx.try_recv() {
                Ok(Ok(updated)) => {
                    if updated > 0 {
                        for artwork in &mut self.deck_artwork {
                            artwork.invalidate();
                        }
                        self.refresh_tracks();
                        changed = true;
                    }
                }
                Ok(Err(error)) => tracing::warn!("artwork backfill: {error}"),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.artwork_rx = Some(rx);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        changed
    }
}
