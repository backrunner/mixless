//! Storage usage scan state and cache controls.
use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ScanState {
    Idle,
    Queued,
    Running,
}

impl Preferences {
    pub(super) fn refresh_storage(&mut self) {
        if self.storage_scan != ScanState::Idle {
            return;
        }
        self.storage_scan = ScanState::Queued;
        self.start_queued_storage_scan();
    }

    pub(super) fn start_queued_storage_scan(&mut self) {
        if self.storage_scan != ScanState::Queued || self.job.is_some() {
            return;
        }
        self.storage_scan = ScanState::Running;
        let core = self.core.clone();
        self.start_job(move || JobResult::Storage(storage::report(&core)));
    }

    fn clear_storage(&mut self, kind: ClearKind) {
        let core = self.core.clone();
        self.status = "Clearing...".into();
        self.error = false;
        self.start_job(move || {
            JobResult::StorageClear(
                storage::clear(&core, kind)
                    .and_then(|freed| storage::report(&core).map(|report| (freed, report))),
            )
        });
    }

    /// A cache row: name and what it holds on the left, size plus an optional
    /// two-step Clear on the right. The first click arms; the second clears.
    fn storage_row(
        &self,
        id: impl Into<SharedString>,
        label: &str,
        detail: &str,
        bytes: u64,
        kind: Option<ClearKind>,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let busy = self.job.is_some();
        let armed = kind.is_some() && self.confirm_clear == kind;
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .min_h(px(56.))
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(theme::LINE_SOFT)
            .text_size(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().child(label.to_string()))
                    .when(!detail.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::MUTED)
                                .child(detail.to_string()),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .flex_none()
                    .child(
                        div()
                            .text_color(theme::MUTED)
                            .child(storage::format_bytes(bytes)),
                    )
                    .when_some(kind, |el, kind| {
                        el.child(
                            self.button(
                                format!("clear-{}", id.into()),
                                if armed { "Confirm clear" } else { "Clear" },
                                !busy && (bytes > 0 || armed),
                                move |s, _, _| {
                                    if s.confirm_clear == Some(kind) {
                                        s.confirm_clear = None;
                                        s.clear_storage(kind);
                                    } else {
                                        s.confirm_clear = Some(kind);
                                    }
                                },
                                cx,
                            )
                            .when(armed, |el| el.text_color(theme::DANGER)),
                        )
                    }),
            )
    }

    pub(super) fn storage_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let busy = self.job.is_some();
        if self.storage.is_none() && self.storage_scan != ScanState::Idle {
            return div()
                .id("storage-loading")
                .debug_selector(|| "storage-loading".into())
                .w_full()
                .min_h(px(280.))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_3()
                .child(widgets::loading_indicator())
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme::MUTED)
                        .child("Calculating storage usage…"),
                )
                .into_any_element();
        }
        let mut page = setting_stack().w_full().max_w(px(760.)).mx_auto()
            .child(div().text_size(px(11.)).line_height(px(16.)).text_color(theme::MUTED)
                .child("Clear cached data to reclaim space. Source audio, playlists and saved cues are kept."));
        let Some(report) = &self.storage else {
            return page
                .child(info("Usage", "Not computed".into()))
                .child(div().flex().justify_end().py_3().child(self.button(
                    "storage-scan",
                    if self.error { "Retry" } else { "Compute usage" },
                    !busy,
                    |s, _, _| {
                        s.result(Ok(()), "");
                        s.refresh_storage();
                    },
                    cx,
                )))
                .into_any_element();
        };
        page = page
            .child(setting_group(
                "Cached data",
                vec![
                    self.storage_row(
                        "stems",
                        "Stem cache",
                        "Separated vocals, drums and instruments",
                        report.stems,
                        Some(ClearKind::Stems),
                        cx,
                    ),
                    self.storage_row(
                        "analysis",
                        "Track analysis",
                        "Beat grids, keys and song structure",
                        report.analysis,
                        Some(ClearKind::Analysis),
                        cx,
                    ),
                    self.storage_row(
                        "waveforms",
                        "Waveforms",
                        "Rebuilt from audio when needed",
                        report.waveforms,
                        Some(ClearKind::Waveforms),
                        cx,
                    ),
                    self.storage_row(
                        "artwork",
                        "Artwork",
                        "Rebuilt on next launch",
                        report.artwork,
                        Some(ClearKind::Artwork),
                        cx,
                    ),
                    self.storage_row(
                        "models",
                        "Analysis models",
                        "Downloaded copies only; models included in the app are kept",
                        report.models,
                        Some(ClearKind::Models),
                        cx,
                    ),
                ],
            ))
            .child(setting_group(
                "Library",
                vec![
                    self.storage_row(
                        "database",
                        "Library database",
                        "Tracks, cues, playlists and analysis",
                        report.database,
                        None,
                        cx,
                    ),
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .min_h(px(56.))
                        .px_4()
                        .py_3()
                        .border_b_1()
                        .border_color(theme::LINE_SOFT)
                        .text_size(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().child("Downloaded audio"))
                                .child(
                                    div().text_size(px(11.)).text_color(theme::MUTED).child(
                                        crate::state::display_dir(&self.core.download_dir()),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .flex_none()
                                .child(
                                    div()
                                        .text_color(theme::MUTED)
                                        .child(storage::format_bytes(report.acquired)),
                                )
                                .child(
                                    self.button(
                                        "storage-acquired",
                                        "Show in Finder",
                                        true,
                                        |s, _, cx| {
                                            let dir = s.core.download_dir();
                                            std::fs::create_dir_all(&dir).ok();
                                            cx.reveal_path(&dir);
                                        },
                                        cx,
                                    )
                                    .into_any_element(),
                                ),
                        ),
                ],
            ));
        if !report.playlists.is_empty() {
            page = page
                .child(setting_group(
                    "Per playlist",
                    report
                        .playlists
                        .iter()
                        .map(|playlist| {
                            self.storage_row(
                                format!("playlist-{}", playlist.id),
                                &playlist.name,
                                &format!("{} tracks", playlist.tracks),
                                playlist.bytes,
                                Some(ClearKind::Playlist(playlist.id)),
                                cx,
                            )
                        })
                        .collect(),
                ))
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(theme::MUTED)
                        .child("Tracks shared with other playlists use the same caches."),
                );
        }
        page.child(div().flex().justify_end().py_3().child(self.button(
            "storage-refresh",
            "Refresh usage",
            !busy,
            |s, _, _| s.refresh_storage(),
            cx,
        )))
        .into_any_element()
    }
}
