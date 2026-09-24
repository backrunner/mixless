//! Per-recording retries have their own bounded queue, independent of imports.
use super::*;
use mixless_library::ImportItem;
use std::collections::{HashSet, VecDeque};
use std::sync::mpsc::TryRecvError;

const WORKERS: usize = 4;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    playlist: i64,
    position: usize,
    external_id: String,
}

impl Key {
    fn new(playlist: i64, item: &ImportItem) -> Self {
        Self {
            playlist,
            position: item.position,
            external_id: item.external_id.clone(),
        }
    }
}

#[derive(Default)]
pub(super) struct Retries {
    pending: HashSet<Key>,
    queue: VecDeque<Key>,
    active: Vec<(Key, Receiver<ImportMsg>)>,
    selections: Vec<(i64, Receiver<Result<Vec<ImportItem>, String>>)>,
    total: usize,
    completed: usize,
    failed: usize,
}

impl Retries {
    pub(super) fn has_playlist(&self, playlist: i64) -> bool {
        self.pending.iter().any(|key| key.playlist == playlist)
            || self.selections.iter().any(|(id, _)| *id == playlist)
    }
    fn enqueue(&mut self, key: Key) -> bool {
        if self.pending.is_empty() {
            self.total = 0;
            self.completed = 0;
            self.failed = 0;
        }
        if !self.pending.insert(key.clone()) {
            return false;
        }
        self.queue.push_back(key);
        self.total += 1;
        true
    }

    fn enqueue_failed(&mut self, playlist: i64, items: &[ImportItem]) {
        for item in items.iter().filter(|item| item.is_failed()) {
            self.enqueue(Key::new(playlist, item));
        }
    }

    fn start(&mut self, mut launch: impl FnMut(Key, Sender<ImportMsg>)) {
        while self.active.len() < WORKERS {
            // Repeated occurrences reuse the first result instead of racing
            // downloads that publish the same recording filename.
            let Some(index) = self.queue.iter().position(|key| {
                !self
                    .active
                    .iter()
                    .any(|(active, _)| active.external_id == key.external_id)
            }) else {
                break;
            };
            let key = self.queue.remove(index).unwrap();
            let (tx, rx) = channel();
            self.active.push((key.clone(), rx));
            launch(key, tx);
        }
    }

    fn poll(&mut self) -> Vec<ImportMsg> {
        let mut messages = Vec::new();
        let active = std::mem::take(&mut self.active);
        for (key, rx) in active {
            let mut done = false;
            loop {
                let message = match rx.try_recv() {
                    Ok(message) => message,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        ImportMsg::Done(Err("Track retry stopped unexpectedly".into()))
                    }
                };
                if let ImportMsg::Done(result) = &message {
                    self.pending.remove(&key);
                    self.completed += 1;
                    if result
                        .as_ref()
                        .map_or(true, |report| report.failed + report.suspect > 0)
                    {
                        self.failed += 1;
                    }
                    done = true;
                }
                messages.push(message);
                if done {
                    break;
                }
            }
            if !done {
                self.active.push((key, rx));
            }
        }
        messages
    }

    pub(super) fn status(&self) -> Option<String> {
        if !self.selections.is_empty() {
            return Some("Finding failed tracks to retry".into());
        }
        if self.total == 0 {
            return None;
        }
        Some(if self.pending.is_empty() {
            format!("Retried {} · {} failed", self.completed, self.failed)
        } else {
            format!(
                "Retrying · {} / {} complete · {} active · {} queued",
                self.completed,
                self.total,
                self.active.len(),
                self.queue.len()
            )
        })
    }
}

impl UiState {
    pub fn retry_import_item(&mut self, playlist: i64, position: usize) {
        if self.playlist_sync_pending(playlist) {
            return;
        }
        if self.playlist_sel != Some(playlist) {
            return;
        }
        let Some(item) = self
            .playlist_imports
            .iter()
            .find(|item| item.position == position && item.is_failed())
        else {
            return;
        };
        if self.import_retries.enqueue(Key::new(playlist, item)) {
            self.library_key = None;
            self.start_import_retries();
        }
    }

    pub fn import_retry_pending(&self, playlist: Option<i64>, item: &ImportItem) -> bool {
        playlist.is_some_and(|id| self.import_retries.pending.contains(&Key::new(id, item)))
    }

    pub fn retry_failed_imports(&mut self, playlist: i64) {
        if self.playlist_sync_pending(playlist) {
            return;
        }
        if self
            .import_retries
            .selections
            .iter()
            .any(|(id, _)| *id == playlist)
        {
            return;
        }
        let (tx, rx) = channel();
        self.import_retries.selections.push((playlist, rx));
        let core = self.core.clone();
        std::thread::spawn(move || {
            let _ = tx.send(
                core.library
                    .import_items(PlaylistId(playlist))
                    .map_err(|e| e.to_string()),
            );
        });
    }

    fn start_import_retries(&mut self) {
        let core = self.core.clone();
        self.import_retries.start(|key, tx| {
            let core = core.clone();
            std::thread::spawn(move || {
                imports::retry_spotify_blocking(
                    &core,
                    PlaylistId(key.playlist),
                    key.position,
                    &key.external_id,
                    &tx,
                )
            });
        });
    }

    pub(super) fn poll_import_retries(&mut self) -> bool {
        let mut changed = false;
        let selections = std::mem::take(&mut self.import_retries.selections);
        for (playlist, rx) in selections {
            match rx.try_recv() {
                Ok(Ok(items)) => {
                    self.import_retries.enqueue_failed(playlist, &items);
                    changed = true;
                }
                Ok(Err(error)) => {
                    self.error = error.into();
                    changed = true;
                }
                Err(TryRecvError::Empty) => self.import_retries.selections.push((playlist, rx)),
                Err(TryRecvError::Disconnected) => {
                    self.error = "Could not load failed tracks for retry".into();
                    changed = true;
                }
            }
        }
        let mut refresh = false;
        for message in self.import_retries.poll() {
            changed = true;
            match message {
                ImportMsg::Done(result) => {
                    refresh = true;
                    match result {
                        Ok(report) => self.import_details.extend(report.errors),
                        Err(error) => self.error = error.into(),
                    }
                }
                ImportMsg::LibraryChanged => refresh = true,
                ImportMsg::Progress(_) | ImportMsg::PlaylistReady(_) => {}
            }
        }
        self.start_import_retries();
        if self.import_rx.is_none() && !self.import_queue.is_empty() {
            self.start_next_import();
        }
        if refresh {
            self.refresh_tracks();
        }
        if changed {
            self.library_key = None;
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn item(position: usize, status: &str) -> ImportItem {
        ImportItem {
            position,
            external_id: ((b'a' + position as u8) as char).to_string().repeat(22),
            title: format!("Retry fixture {position}"),
            artist: "Retry artist".into(),
            duration_ms: 2000,
            status: status.into(),
            track_id: None,
            error: Some("offline".into()),
        }
    }

    #[test]
    fn independent_retries_run_concurrently_with_a_bound_and_batch_deduplication() {
        let (_dir, core, _) = crate::automix::tests::fixture();
        let pid = core
            .library
            .external_playlist("spotify", "retry-pool", "Retry pool")
            .unwrap();
        let items: Vec<_> = (0..6).map(|i| item(i, "missing")).collect();
        core.library.save_import_items(pid, &items).unwrap();
        let mut retries = Retries::default();
        let (entered_tx, entered_rx) = channel();
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        std::thread::scope(|scope| {
            let launch = |key: Key, tx: Sender<ImportMsg>| {
                let core = &core;
                let entered = entered_tx.clone();
                let gate = gate.clone();
                scope.spawn(move || {
                    let service = ImportService {
                        library: &core.library,
                        analyzer: &core.analyzer,
                    };
                    let result = service.retry_spotify_item(
                        PlaylistId(key.playlist),
                        key.position,
                        &key.external_id,
                        |_, _| {
                            entered.send(key.position).unwrap();
                            let (lock, wake) = &*gate;
                            let (_guard, timeout) = wake
                                .wait_timeout_while(
                                    lock.lock().unwrap(),
                                    Duration::from_secs(5),
                                    |open| !*open,
                                )
                                .unwrap();
                            assert!(!timeout.timed_out(), "retries did not overlap");
                            Err(mixless_acquire::AcquireError::Msg(
                                "fixture still unavailable".into(),
                            ))
                        },
                        |_| {},
                    );
                    tx.send(ImportMsg::Done(result)).unwrap();
                });
            };
            assert!(retries.enqueue(Key::new(pid.0, &items[0])));
            retries.start(&launch);
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            // Starting one row must not disable the other single-row retries.
            assert!(retries.enqueue(Key::new(pid.0, &items[1])));
            retries.start(&launch);
            entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            // Retry all, even from a stale snapshot, shares this same queue.
            retries.enqueue_failed(pid.0, &items);
            retries.start(&launch);
            for _ in 0..2 {
                entered_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
            assert!(entered_rx.recv_timeout(Duration::from_millis(50)).is_err());
            assert_eq!(
                (retries.active.len(), retries.queue.len(), retries.total),
                (4, 2, 6)
            );
            assert_eq!(
                core.library
                    .import_items(pid)
                    .unwrap()
                    .iter()
                    .filter(|i| i.status == "resolving")
                    .count(),
                4
            );
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
        });
        assert_eq!(retries.poll().len(), 4);
        assert_eq!(
            (retries.completed, retries.failed, retries.pending.len()),
            (4, 4, 2)
        );
        let mut next = Vec::new();
        retries.start(|key, tx| next.push((key, tx)));
        assert_eq!(
            next.iter().map(|(key, _)| key.position).collect::<Vec<_>>(),
            [4, 5]
        );
        for (_, tx) in next {
            tx.send(ImportMsg::Done(Ok(ImportReport {
                acquired: 1,
                ..Default::default()
            })))
            .unwrap();
        }
        retries.poll();
        assert!(retries.pending.is_empty());
        assert_eq!((retries.completed, retries.failed), (6, 4));
    }

    #[test]
    fn retry_all_filters_rows_and_duplicate_recordings_wait_without_blocking_other_tracks() {
        let mut retries = Retries::default();
        let mut items = vec![
            item(0, "missing"),
            item(1, "acquired"),
            item(2, "queued"),
            item(3, "suspect"),
        ];
        retries.enqueue_failed(7, &items);
        retries.enqueue_failed(7, &items);
        assert_eq!(retries.pending.len(), 2);
        items[1].status = "missing".into();
        items[1].external_id = items[0].external_id.clone();
        retries.enqueue_failed(7, &items);
        let mut running = Vec::new();
        retries.start(|key, tx| running.push((key, tx)));
        assert_eq!(
            running
                .iter()
                .map(|(key, _)| key.position)
                .collect::<Vec<_>>(),
            [0, 3]
        );
        assert_eq!(retries.queue.len(), 1);
        // One failed/disconnected worker releases only its own slot/identity.
        drop(running.remove(0));
        retries.poll();
        assert!(!retries.pending.contains(&Key::new(7, &items[0])));
        assert!(retries.pending.contains(&Key::new(7, &items[3])));
        retries.start(|key, tx| running.push((key, tx)));
        assert_eq!(running[1].0.position, 1);
        assert_eq!(retries.failed, 1);
    }
}
