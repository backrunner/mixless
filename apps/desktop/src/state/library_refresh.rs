//! Coalesced library reads. No SQLite waits in a display frame or pointer handler.
use super::*;

struct LibraryRows {
    selection: Option<i64>,
    order_revision: u64,
    tracks: Vec<Track>,
    playlists: Vec<mixless_library::PlaylistSummary>,
    issues: Vec<mixless_library::ImportItem>,
}

pub(super) struct LibraryRefresh {
    rx: Option<Receiver<Result<LibraryRows, String>>>,
    again: bool,
    pub initial: bool,
}
impl Default for LibraryRefresh {
    fn default() -> Self {
        Self {
            rx: None,
            again: false,
            initial: true,
        }
    }
}

impl UiState {
    pub fn refresh_playlists(&mut self) {
        self.refresh_tracks();
    }

    pub fn refresh_tracks(&mut self) {
        if self.library_refresh.rx.is_some() {
            self.library_refresh.again = true;
            return;
        }
        let selection = self.playlist_sel;
        let order_revision = self.library_order.revision;
        let core = self.core.clone();
        let (tx, rx) = channel();
        self.library_refresh.rx = Some(rx);
        std::thread::spawn(move || {
            let result = (|| {
                let playlists = core.library.list_playlists().map_err(|e| e.to_string())?;
                let tracks = match selection {
                    Some(id) => core.library.playlist_tracks(PlaylistId(id)),
                    None => core.library.list_tracks(),
                }
                .map_err(|e| e.to_string())?;
                let issues = selection
                    .and_then(|id| core.library.import_items(PlaylistId(id)).ok())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|item| !matches!(item.status.as_str(), "local" | "acquired"))
                    .collect();
                Ok(LibraryRows {
                    selection,
                    order_revision,
                    tracks,
                    playlists,
                    issues,
                })
            })();
            let _ = tx.send(result);
        });
    }

    pub(super) fn poll_library(&mut self) -> bool {
        let Some(rx) = self.library_refresh.rx.take() else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(mut rows)) => {
                self.playlists = Arc::new(rows.playlists);
                if self.library_refresh.initial {
                    self.library_refresh.initial = false;
                    let saved = self.core.settings.get();
                    if !saved.library_selection_saved {
                        if let Some(id) = self.playlists.first().map(|pl| pl.id) {
                            self.select_playlist(Some(id));
                        }
                    } else if saved
                        .last_playlist
                        .is_some_and(|id| !self.playlists.iter().any(|pl| pl.id == id))
                    {
                        self.select_playlist(None);
                    }
                }
                if rows.selection == self.playlist_sel
                    && rows.order_revision == self.library_order.revision
                    && !self.library_order.is_pending()
                {
                    // Analysis can finish after this query starts. Do not
                    // replace fresh row metadata with an older query result.
                    if let Ok(latest) = self.core.analysis.latest.lock() {
                        for track in &mut rows.tracks {
                            if let Some(updated) = latest.get(&track.id).filter(|updated| {
                                !track.analyzed
                                    && (track.content_hash.is_empty()
                                        || track.content_hash == updated.content_hash)
                            }) {
                                *track = updated.clone();
                            }
                        }
                    }
                    crate::analysis::schedule(&self.core, &rows.tracks);
                    crate::automix::prepare_playlist(
                        &self.core,
                        rows.tracks.iter().map(|t| t.id).collect(),
                    );
                    self.tracks = Arc::new(rows.tracks);
                    self.playlist_issues = rows.issues;
                } else {
                    self.library_refresh.again = true;
                }
            }
            Ok(Err(error)) => self.error = error.into(),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                self.library_refresh.rx = Some(rx);
                return false;
            }
            Err(_) => self.error = "Library refresh failed".into(),
        }
        if std::mem::take(&mut self.library_refresh.again) {
            self.refresh_tracks();
        }
        true
    }
}
