//! Optimistic row moves with ordered background writes and stale-read protection.
use super::*;
use crate::views::library::TrackDrag;
use std::collections::VecDeque;

struct OrderRequest {
    playlist: Option<i64>,
    expected: Vec<TrackId>,
    ordered: Vec<TrackId>,
}

#[derive(Default)]
pub(super) struct LibraryOrder {
    pub revision: u64,
    pending: VecDeque<OrderRequest>,
    rx: Option<Receiver<Result<(), String>>>,
}

impl LibraryOrder {
    pub fn is_pending(&self) -> bool {
        self.rx.is_some() || !self.pending.is_empty()
    }

    fn finish(mut self, core: &AppCore) -> Result<(), String> {
        if let Some(rx) = self.rx.take() {
            rx.recv().map_err(|e| e.to_string())??;
        }
        for request in self.pending {
            core.library
                .reorder_tracks(
                    request.playlist.map(PlaylistId),
                    &request.expected,
                    &request.ordered,
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

/// Insertion slots are measured before removing the source row.
fn destination(len: usize, from: usize, insertion: usize) -> Option<usize> {
    if from >= len || insertion > len {
        return None;
    }
    let to = insertion - usize::from(insertion > from);
    (from != to).then_some(to)
}

impl UiState {
    pub(super) fn flush_library_order(&mut self) {
        let order = std::mem::take(&mut self.library_order);
        if !order.is_pending() {
            return;
        }
        let core = self.core.clone();
        // GPUI only grants quit futures 100 ms. Finish durable writes before
        // returning that future, or a quick Cmd-Q can discard queued row moves.
        // This wait runs only during application shutdown, never in a frame.
        match std::thread::spawn(move || order.finish(&core)).join() {
            Ok(Ok(())) => {}
            result => tracing::error!(?result, "Could not finish playlist order on exit"),
        }
    }

    pub fn reorder_track(&mut self, drag: &TrackDrag, insertion: usize) {
        if drag.playlist != self.playlist_sel
            || !drag
                .tracks
                .iter()
                .map(|t| t.id)
                .eq(self.tracks.iter().map(|t| t.id))
        {
            return;
        }
        let Some(to) = destination(self.tracks.len(), drag.index, insertion) else {
            return;
        };
        let expected = self.tracks.iter().map(|track| track.id).collect();
        let tracks = Arc::make_mut(&mut self.tracks);
        let track = tracks.remove(drag.index);
        tracks.insert(to, track);
        let ordered: Vec<_> = tracks.iter().map(|track| track.id).collect();
        crate::automix::prepare_playlist(&self.core, ordered.clone());
        self.track_sel = Some(drag.id.0);
        self.track_menu = None;
        self.library_key = None;
        self.library_order.revision += 1;
        self.library_order.pending.push_back(OrderRequest {
            playlist: self.playlist_sel,
            expected,
            ordered,
        });
        self.start_library_order();
    }

    fn start_library_order(&mut self) {
        if self.library_order.rx.is_some() {
            return;
        }
        let Some(request) = self.library_order.pending.pop_front() else {
            return;
        };
        let core = self.core.clone();
        let (tx, rx) = channel();
        self.library_order.rx = Some(rx);
        std::thread::spawn(move || {
            let result = core
                .library
                .reorder_tracks(
                    request.playlist.map(PlaylistId),
                    &request.expected,
                    &request.ordered,
                )
                .map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
    }

    pub(super) fn poll_library_order(&mut self) -> bool {
        let Some(rx) = self.library_order.rx.take() else {
            return false;
        };
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.library_order.rx = Some(rx);
                return false;
            }
            Err(_) => Err("Could not save track order".into()),
        };
        self.library_order.revision += 1;
        if let Err(error) = result {
            self.error = error.into();
            self.library_order.pending.clear();
        }
        self.start_library_order();
        if !self.library_order.is_pending() {
            self.refresh_tracks();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::destination;

    #[test]
    fn shutdown_drains_moves_after_a_slow_write_without_another_ui_poll() {
        use super::*;
        let (dir, core, tracks) = crate::automix::tests::fixture();
        let playlist = core.library.replace_playlist("Set", &tracks).unwrap();
        let first = vec![tracks[1], tracks[0], tracks[2]];
        let final_order = vec![tracks[2], tracks[1], tracks[0]];
        let (tx, rx) = channel();
        let worker = core.clone();
        let before = tracks.clone();
        let after = first.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            tx.send(
                worker
                    .library
                    .reorder_tracks(Some(playlist), &before, &after)
                    .map_err(|e| e.to_string()),
            )
            .unwrap();
        });
        let mut order = LibraryOrder {
            rx: Some(rx),
            ..Default::default()
        };
        order.pending.push_back(OrderRequest {
            playlist: Some(playlist.0),
            expected: first,
            ordered: final_order.clone(),
        });
        order.finish(&core).unwrap();
        let reopened = mixless_library::Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(
            reopened
                .playlist_tracks(playlist)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            final_order
        );
    }

    #[test]
    fn whole_row_moves_use_insertion_slots_in_both_directions() {
        let moved = |from, insertion| {
            let mut rows = vec!["A", "B", "C", "D"];
            if let Some(to) = destination(rows.len(), from, insertion) {
                let row = rows.remove(from);
                rows.insert(to, row);
            }
            rows
        };
        assert_eq!(moved(0, 4), ["B", "C", "D", "A"]);
        assert_eq!(moved(3, 0), ["D", "A", "B", "C"]);
        assert_eq!(moved(1, 3), ["A", "C", "B", "D"]);
        assert_eq!(moved(2, 1), ["A", "C", "B", "D"]);
        assert_eq!(moved(1, 1), ["A", "B", "C", "D"]);
        assert_eq!(moved(1, 2), ["A", "B", "C", "D"]);
        assert_eq!(destination(0, 0, 0), None);
        assert_eq!(destination(4, 4, 0), None);
        assert_eq!(destination(4, 1, 5), None);
    }
}
