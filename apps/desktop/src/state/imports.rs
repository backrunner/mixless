//! File pickers, import queue and background acquisition.
use super::*;

impl UiState {
    /// Minimal single-line input handling: printable keys, backspace, paste,
    /// escape/enter to blur. No IME in v1.
    pub fn handle_url_input_key(
        &mut self,
        ks: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ks.key.as_str() {
            "escape" => {
                self.show_import_modal = false;
                window.blur();
            }
            "enter" => {
                if !self.url.trim().is_empty() {
                    let url = self.url.trim().to_string();
                    self.import_spotify(url);
                }
                window.blur();
            }
            "backspace" => {
                self.url.pop();
            }
            _ => {
                if ks.modifiers.platform && ks.key == "v" {
                    if let Some(item) = cx.read_from_clipboard()
                        && let Some(text) = item.text()
                    {
                        let text = text.replace(['\n', '\r'], " ");
                        self.url.push_str(&text);
                    }
                } else if !ks.modifiers.control && !ks.modifiers.platform && !ks.modifiers.alt {
                    if let Some(ch) = &ks.key_char
                        && !ch.chars().any(|c| c.is_control())
                    {
                        self.url.push_str(ch);
                    }
                }
            }
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

    pub fn import_spotify(&mut self, url: String) {
        self.import_queue.push_back(ImportRequest::Spotify(url));
        self.start_next_import();
    }

    pub fn import_status(&self) -> SharedString {
        if self.import_queue.is_empty() {
            return self.acquire.clone();
        }
        format!(
            "{} · {} imports queued",
            self.acquire,
            self.import_queue.len()
        )
        .into()
    }

    pub(super) fn start_next_import(&mut self) {
        if self.import_rx.is_some() {
            return;
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
        }
        .into();
        let (tx, rx) = channel();
        self.import_rx = Some(rx);
        let core = self.core.clone();
        std::thread::spawn(move || match request {
            ImportRequest::Local(paths) => import_files_blocking(&core, paths, &tx),
            ImportRequest::Spotify(url) => import_spotify_blocking(&core, &url, &tx),
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

fn import_spotify_blocking(core: &AppCore, url: &str, tx: &Sender<ImportMsg>) {
    let result = (|| -> Result<ImportReport, String> {
        let playlist = SpotifyClient::new()
            .fetch_playlist(url)
            .map_err(|e| e.to_string())?;
        let service = ImportService {
            library: &core.library,
            analyzer: &core.analyzer,
        };
        let mut provider = None;
        service.spotify_playlist(
            &playlist,
            |job| {
                // Local-only imports work even if no downloader is installed.
                if provider.is_none() {
                    provider = Some(YoutubeMusicAcquire::detect()?);
                }
                provider.as_ref().unwrap().fetch(job, &core.acquired_dir)
            },
            |message| {
                let _ = tx.send(ImportMsg::Progress(message));
            },
        )
    })();
    let _ = tx.send(ImportMsg::Done(result));
}
