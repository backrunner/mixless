use super::*;
use std::sync::atomic::AtomicBool;

mod selection;
pub(crate) use selection::{CheckState, PackageNode};

#[derive(Clone, Copy, PartialEq)]
pub enum PackageAction {
    Import,
    Export,
    Append,
}
enum Message {
    Progress(String),
    Done(Result<mixless_pack::Report, String>),
}
pub struct PackageUi {
    receiver: Option<Receiver<Message>>,
    cancel: Arc<AtomicBool>,
    pub status: String,
    pub all: bool,
    pub selected: std::collections::BTreeSet<i64>,
    pub export_open: bool,
    pub menu: Option<gpui::Point<gpui::Pixels>>,
    pub expanded: [bool; 3],
    pub focus: gpui::FocusHandle,
    pub action: Option<PackageAction>,
}
impl PackageUi {
    pub fn new(cx: &mut Context<UiState>) -> Self {
        Self {
            all: true,
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            status: String::new(),
            selected: Default::default(),
            export_open: false,
            menu: None,
            expanded: [true; 3],
            focus: cx.focus_handle().tab_index(0).tab_stop(false),
            action: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.receiver.is_some()
    }
}

impl UiState {
    pub fn open_package_export(&mut self, playlist: Option<i64>) {
        if !self.package.busy() && !self.picker_open {
            self.package.all = playlist.is_none();
            self.package.selected = playlist.into_iter().collect();
            self.package.expanded = [true; 3];
            self.package.status.clear();
            self.package.action = None;
        }
        self.show_import_modal = false;
        self.package.export_open = true;
        self.package.menu = None;
        self.playlist_menu = None;
        self.track_menu = None;
        self.deck_menu = None;
        self.brand_menu.open = false;
        self.audio.open = false;
    }

    pub fn cancel_package(&mut self) {
        self.package.cancel.store(true, Ordering::Release);
    }

    pub fn choose_package(&mut self, action: PackageAction, cx: &mut Context<Self>) {
        if self.picker_open || self.package.busy() {
            return;
        }
        let selection = if self.package.all {
            mixless_pack::Selection::Library
        } else {
            if action != PackageAction::Import && self.package.selected.is_empty() {
                self.error = "Select playlists or Entire library".into();
                return;
            }
            mixless_pack::Selection::Playlists(
                self.package
                    .selected
                    .iter()
                    .map(|id| PlaylistId(*id))
                    .collect(),
            )
        };
        self.flush_library_order();
        self.picker_open = true;
        cx.spawn(async move |this, cx| {
            let dialog = rfd::AsyncFileDialog::new().add_filter("mixless package", &["mixpack"]);
            let file = match action {
                PackageAction::Import => dialog.set_title("Import package").pick_file().await,
                PackageAction::Append => {
                    dialog
                        .set_title("Update existing package")
                        .pick_file()
                        .await
                }
                PackageAction::Export => {
                    dialog
                        .set_title("Export package")
                        .set_file_name("Library.mixpack")
                        .save_file()
                        .await
                }
            };
            let _ = this.update(cx, |s, cx| {
                s.picker_open = false;
                if let Some(file) = file {
                    let path = file.path().to_path_buf();
                    s.start_package(path, action, selection);
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    fn start_package(
        &mut self,
        path: PathBuf,
        action: PackageAction,
        selection: mixless_pack::Selection,
    ) {
        let (tx, rx) = channel();
        self.package.receiver = Some(rx);
        self.package.action = Some(action);
        self.package.cancel = Arc::new(AtomicBool::new(false));
        self.package.status = "Preparing package…".into();
        self.error = "".into();
        let cancel = self.package.cancel.clone();
        let core = self.core.clone();
        std::thread::spawn(move || {
            let result = (|| -> Result<_, String> {
                let stems = core.stems.as_ref().ok_or("Stem cache unavailable")?;
                let active = || {
                    !cancel.load(Ordering::Acquire) && !core.shutting_down.load(Ordering::Acquire)
                };
                let progress = |status| {
                    let _ = tx.send(Message::Progress(status));
                };
                if action == PackageAction::Import {
                    let report =
                        mixless_pack::import(&core.library, stems, &path, progress, active)
                            .map_err(|e| e.to_string())?;
                    for id in &report.imported {
                        core.analysis.forget(*id);
                    }
                    Ok(report)
                } else if action == PackageAction::Export {
                    // Save As publishes by rename, retaining the previous file
                    // until a complete replacement is ready. Append edits in place.
                    let parent = path.parent().unwrap_or(std::path::Path::new("."));
                    let temp =
                        tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
                    let report = mixless_pack::export(
                        &core.library,
                        stems,
                        temp.path(),
                        selection,
                        progress,
                        active,
                    )
                    .map_err(|e| e.to_string())?;
                    temp.persist(&path).map_err(|e| e.to_string())?;
                    #[cfg(unix)]
                    std::fs::File::open(parent)
                        .and_then(|dir| dir.sync_all())
                        .map_err(|e| e.to_string())?;
                    Ok(report)
                } else {
                    mixless_pack::export(&core.library, stems, &path, selection, progress, active)
                        .map_err(|e| e.to_string())
                }
            })();
            let _ = tx.send(Message::Done(result));
        });
    }

    pub(super) fn poll_package(&mut self) -> bool {
        let Some(rx) = self.package.receiver.take() else {
            return false;
        };
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok(Message::Progress(status)) => {
                    self.package.status = status;
                    changed = true;
                }
                Ok(Message::Done(result)) => {
                    match result {
                        Ok(report) => {
                            self.package.status = format!(
                                "{} {} playlists, {} tracks ({} with stems)",
                                match self.package.action {
                                    Some(PackageAction::Import) => "Imported",
                                    Some(PackageAction::Append) => "Updated",
                                    _ => "Exported",
                                },
                                report.playlists,
                                report.tracks,
                                report.stems
                            );
                            if report.without_analysis > 0 || report.without_stems > 0 {
                                self.package.status.push_str(&format!(
                                    "\n{} without analysis, {} without stems",
                                    report.without_analysis, report.without_stems
                                ));
                            }
                            for id in report.imported {
                                self.library_previews.invalidate(id);
                            }
                            self.refresh_tracks();
                            self.refresh_playlists();
                            crate::automix::refresh_previews(&self.core);
                        }
                        Err(error) => {
                            self.package.status = error.clone();
                            self.error = error.into();
                        }
                    }
                    return true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.package.receiver = Some(rx);
                    return changed;
                }
                Err(_) => {
                    self.package.status = "Package worker stopped unexpectedly".into();
                    return true;
                }
            }
        }
    }
}
