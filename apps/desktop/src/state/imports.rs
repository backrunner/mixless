//! File pickers, import queue and background acquisition.
use super::*;

impl UiState {
    /// Single-line URL editing, including selection and explicit blur.
    pub fn handle_url_input_key(
        &mut self,
        ks: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let selection = &mut self.url_selection;
        match ks.key.as_str() {
            "escape" | "tab" => self.keyboard_focus.focus(window),
            "enter" => {
                if !self.url.trim().is_empty() {
                    self.import_spotify(self.url.trim().to_string());
                }
                self.keyboard_focus.focus(window);
            }
            "left" | "right" | "home" | "end" => {
                let right = matches!(ks.key.as_str(), "right" | "end");
                let cursor = if ks.modifiers.platform || matches!(ks.key.as_str(), "home" | "end") {
                    if right { self.url.len() } else { 0 }
                } else if !ks.modifiers.shift && !selection.range().is_empty() {
                    if right {
                        selection.range().end
                    } else {
                        selection.range().start
                    }
                } else {
                    selection.adjacent(&self.url, right)
                };
                selection.move_to(cursor, ks.modifiers.shift);
            }
            "backspace" | "delete" => {
                if selection.range().is_empty() {
                    let cursor = if ks.modifiers.platform {
                        0
                    } else {
                        selection.adjacent(&self.url, ks.key == "delete")
                    };
                    selection.move_to(cursor, true);
                }
                selection.replace(&mut self.url, "");
            }
            "a" if ks.modifiers.platform => {
                selection.move_to(0, false);
                selection.move_to(self.url.len(), true);
            }
            "c" | "x" if ks.modifiers.platform => {
                let range = selection.range();
                if !range.is_empty() {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                        self.url[range].to_string(),
                    ));
                    if ks.key == "x" {
                        selection.replace(&mut self.url, "");
                    }
                }
            }
            "v" if ks.modifiers.platform => {
                if let Some(item) = cx.read_from_clipboard()
                    && let Some(text) = item.text()
                {
                    selection.replace(&mut self.url, &text.replace(['\n', '\r'], " "));
                }
            }
            _ if !ks.modifiers.control && !ks.modifiers.platform && !ks.modifiers.alt => {
                if let Some(ch) = &ks.key_char
                    && !ch.chars().any(|c| c.is_control())
                {
                    selection.replace(&mut self.url, ch);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    /* ---- library --------------------------------------------------------- */

    pub fn choose_local(&mut self, folders: bool, cx: &mut Context<Self>) {
        if self.picker_open {
            return;
        }
        self.picker_open = true;
        let selection = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: Some(
                if folders {
                    "Import folders"
                } else {
                    "Import audio files"
                }
                .into(),
            ),
        });
        cx.spawn(async move |this, cx| {
            let result = selection.await;
            let _ = this.update(cx, |s, cx| {
                s.picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        s.show_import_modal = false;
                        s.import_files(paths);
                    }
                    Ok(Err(error)) => s.error = error.to_string().into(),
                    Err(error) => s.error = error.to_string().into(),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn import_files(&mut self, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        // Publish selected directories immediately, including queued and empty ones.
        let mut first = None;
        for path in &paths {
            if path.is_dir() {
                match self.core.library.register_folder(path) {
                    Ok(id) => {
                        first.get_or_insert(id);
                    }
                    Err(error) => self.error = error.to_string().into(),
                }
            }
        }
        self.refresh_playlists();
        if let Some(id) = first {
            self.select_playlist(Some(id.0));
        }
        self.import_queue.push_back(ImportRequest::Local(paths));
        self.start_next_import();
    }

    /// Pick the folder that downloaded Spotify audio is written to. The choice
    /// is stored in preferences and applies to subsequent imports.
    pub fn choose_download_dir(&mut self, cx: &mut Context<Self>) {
        if self.picker_open {
            return;
        }
        self.picker_open = true;
        let selection = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Save downloaded audio to".into()),
        });
        cx.spawn(async move |this, cx| {
            let result = selection.await;
            let _ = this.update(cx, |s, cx| {
                s.picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(dir) = paths.into_iter().next()
                            && let Err(error) = ensure_writable_dir(&dir).and_then(|()| {
                                s.core.settings.update(|p| p.download_dir = Some(dir))
                            })
                        {
                            s.error = error.into();
                        }
                    }
                    Ok(Err(error)) => s.error = error.to_string().into(),
                    Err(error) => s.error = error.to_string().into(),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn import_spotify(&mut self, url: String) {
        if let Some(id) = mixless_spotify::extract_playlist_id(&url) {
            if self.active_spotify.as_deref() == Some(&id)
                || self.import_queue.iter().any(|request| match request {
                    ImportRequest::Spotify(url) => {
                        mixless_spotify::extract_playlist_id(url).as_deref() == Some(id.as_str())
                    }
                    ImportRequest::SpotifySync(_, remote) => remote == &id,
                    _ => false,
                })
            {
                return;
            }
            if let Some(playlist) = self
                .playlists
                .iter()
                .find(|p| p.spotify_id.as_deref() == Some(&id))
            {
                self.refresh_spotify_playlist(playlist.id);
                return;
            }
        }
        self.show_import_modal = false;
        self.import_details.clear();
        self.import_queue.push_back(ImportRequest::Spotify(url));
        self.start_next_import();
    }

    pub fn playlist_sync_pending(&self, playlist: i64) -> bool {
        self.active_playlist_sync == Some(playlist)
            || self.import_queue.iter().any(
                |request| matches!(request, ImportRequest::SpotifySync(id, _) if *id == playlist),
            )
    }

    pub fn refresh_spotify_playlist(&mut self, playlist: i64) {
        if self.playlist_sync_pending(playlist) {
            return;
        }
        let Some(remote) = self
            .playlists
            .iter()
            .find(|p| p.id == playlist)
            .and_then(|p| p.spotify_id.clone())
        else {
            return;
        };
        self.show_import_modal = false;
        self.import_details.clear();
        self.import_queue
            .push_back(ImportRequest::SpotifySync(playlist, remote));
        self.start_next_import();
    }

    pub fn import_status(&self) -> SharedString {
        let mut parts = vec![self.acquire.to_string()];
        if !self.package.status.is_empty() {
            parts.push(self.package.status.clone());
        }
        if !self.import_queue.is_empty() {
            parts.push(format!("{} imports queued", self.import_queue.len()));
        }
        if let Some(status) = self.import_retries.status() {
            parts.push(status);
        }
        parts
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ")
            .into()
    }

    pub(super) fn start_next_import(&mut self) {
        if self.import_rx.is_some() {
            return;
        }
        if let Some(ImportRequest::SpotifySync(id, _)) = self.import_queue.front() {
            if self.import_retries.has_playlist(*id) {
                return;
            }
        }
        let Some(request) = self.import_queue.pop_front() else {
            self.busy = false;
            return;
        };
        self.busy = true;
        self.error = "".into();
        self.acquire = match &request {
            ImportRequest::Local(_) => "Finding audio files",
            ImportRequest::Spotify(_) => "Reading playlist",
            ImportRequest::SpotifySync(_, _) => "Updating Spotify playlist",
        }
        .into();
        let (tx, rx) = channel();
        self.import_rx = Some(rx);
        self.active_spotify = match &request {
            ImportRequest::Spotify(url) => mixless_spotify::extract_playlist_id(url),
            ImportRequest::SpotifySync(_, remote) => Some(remote.clone()),
            _ => None,
        };
        self.active_playlist_sync = match &request {
            ImportRequest::SpotifySync(id, _) => Some(*id),
            _ => None,
        };
        let core = self.core.clone();
        std::thread::spawn(move || match request {
            ImportRequest::Local(paths) => import_files_blocking(&core, paths, &tx),
            ImportRequest::Spotify(url) => import_spotify_blocking(&core, &url, None, &tx),
            ImportRequest::SpotifySync(id, remote) => {
                import_spotify_blocking(&core, &remote, Some(PlaylistId(id)), &tx)
            }
        });
    }
}
fn import_files_blocking(core: &Arc<AppCore>, paths: Vec<PathBuf>, tx: &Sender<ImportMsg>) {
    let roots: Vec<_> = paths
        .iter()
        .filter(|p| p.is_dir())
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    let mut report = ImportReport::default();
    let mut batch = Vec::new();
    let mut ids = Vec::new();
    let mut publish = |batch: &mut Vec<PathBuf>| {
        if batch.is_empty() {
            return;
        }
        report.total += batch.len();
        match core.library.register_local_files_into(batch, &roots) {
            Ok(added) => ids.extend(added),
            Err(_) => {
                // A file removed during scanning must not hide its healthy siblings.
                for path in batch.iter() {
                    match core
                        .library
                        .register_local_files_into(std::slice::from_ref(path), &roots)
                    {
                        Ok(added) => ids.extend(added),
                        Err(error) => {
                            report.failed += 1;
                            report.errors.push(format!("{}: {error}", path.display()));
                        }
                    }
                }
            }
        }
        batch.clear();
        let _ = tx.send(ImportMsg::LibraryChanged);
        let _ = tx.send(ImportMsg::Progress(format!(
            "Scanning folder · {} files",
            report.total
        )));
    };
    let mut last_publish = std::time::Instant::now();
    let errors = mixless_acquire::local_paths::visit_audio(&paths, |path| {
        batch.push(path);
        if batch.len() >= 64 || last_publish.elapsed() >= std::time::Duration::from_millis(50) {
            publish(&mut batch);
            last_publish = std::time::Instant::now();
        }
    });
    publish(&mut batch);
    report.errors.extend(errors);
    report.local = ids.len();
    for id in ids {
        core.analysis.forget(id);
    }
    match core.library.list_tracks() {
        Ok(tracks) => crate::analysis::schedule(core, &tracks),
        Err(error) => report.errors.push(error.to_string()),
    }
    let _ = tx.send(ImportMsg::Done(Ok(report)));
}

fn import_spotify_blocking(
    core: &AppCore,
    url: &str,
    sync: Option<PlaylistId>,
    tx: &Sender<ImportMsg>,
) {
    let result = (|| -> Result<ImportReport, String> {
        let client = SpotifyClient::new();
        let playlist = if sync.is_some() {
            client.fetch_complete_playlist(url)
        } else {
            client.fetch_playlist(url)
        }
        .map_err(|e| e.to_string())?;
        let service = ImportService {
            library: &core.library,
            analyzer: &core.analyzer,
        };
        let dest = mixless_acquire::playlist_download_dir(&core.download_dir(), &playlist.name);
        let provider = std::sync::OnceLock::new();
        let fetch = |job: &mixless_acquire::ResolveJob| {
            // Local-only imports work even if no downloader is installed.
            match provider.get_or_init(YoutubeMusicAcquire::detect) {
                Ok(provider) => provider.fetch(job, &dest),
                Err(error) => Err(mixless_acquire::AcquireError::Msg(error.to_string())),
            }
        };
        let progress = |event| publish_import_progress(tx, event);
        if let Some(pid) = sync {
            service.sync_spotify_playlist(pid, &playlist, fetch, progress)
        } else {
            service.spotify_playlist(&playlist, fetch, progress)
        }
    })();
    let _ = tx.send(ImportMsg::Done(result));
}

pub(super) fn retry_spotify_blocking(
    core: &AppCore,
    playlist: PlaylistId,
    position: usize,
    external_id: &str,
    tx: &Sender<ImportMsg>,
) {
    let service = ImportService {
        library: &core.library,
        analyzer: &core.analyzer,
    };
    let provider = std::sync::OnceLock::new();
    let result = service.retry_spotify_item(
        playlist,
        position,
        external_id,
        |job, name| {
            let dest = mixless_acquire::playlist_download_dir(&core.download_dir(), name);
            match provider.get_or_init(YoutubeMusicAcquire::detect) {
                Ok(provider) => provider.fetch(job, &dest),
                Err(error) => Err(mixless_acquire::AcquireError::Msg(error.to_string())),
            }
        },
        |event| publish_import_progress(tx, event),
    );
    let _ = tx.send(ImportMsg::Done(result));
}

fn publish_import_progress(
    tx: &Sender<ImportMsg>,
    event: mixless_acquire::imports::ImportProgress,
) {
    use mixless_acquire::imports::ImportProgress;
    match event {
        ImportProgress::PlaylistReady(id) => {
            let _ = tx.send(ImportMsg::PlaylistReady(id));
        }
        ImportProgress::Changed(_, message) => {
            let _ = tx.send(ImportMsg::Progress(message));
            let _ = tx.send(ImportMsg::LibraryChanged);
        }
    }
}
