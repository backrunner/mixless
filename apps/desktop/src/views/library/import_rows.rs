//! Unresolved recordings occupy real table rows, but never become TrackIds.
use super::*;
use mixless_library::ImportItem;

#[derive(Debug, PartialEq)]
pub(crate) enum Row {
    Track(usize),
    Import(usize),
}

pub(crate) fn rows(track_count: usize, imports: &[ImportItem]) -> Vec<Row> {
    let mut rows = Vec::with_capacity(track_count + imports.len());
    let mut next_track = 0;
    for (index, item) in imports.iter().enumerate() {
        if item.is_ready() && next_track < track_count {
            rows.push(Row::Track(next_track));
            next_track += 1;
        } else if !item.is_ready() {
            rows.push(Row::Import(index));
        }
    }
    rows.extend((next_track..track_count).map(Row::Track));
    rows
}

pub(super) fn element(
    item: &ImportItem,
    row_index: usize,
    compact: bool,
    playlist: Option<i64>,
    retrying: bool,
    state: gpui::Entity<UiState>,
) -> gpui::AnyElement {
    let pending = item.is_pending() || retrying;
    let label = if retrying && item.is_failed() {
        "Retrying · Queued"
    } else {
        match item.status.as_str() {
            "queued" => "Importing · Queued",
            "resolving" => "Importing · Finding / downloading audio",
            "analyzing" => "Importing · Analyzing audio",
            _ => "Import failed",
        }
    };
    let detail = if pending {
        label.into()
    } else {
        item.error.clone().unwrap_or_else(|| label.into())
    };
    let position = item.position;
    let color = if pending { theme::TEXT } else { theme::DANGER };
    gpui::div()
        .id(SharedString::from(format!("import-row-{position}")))
        .relative()
        .overflow_hidden()
        .w_full()
        .flex()
        .items_center()
        .h(px(40.))
        .px(px(ROW_PAD))
        .gap_2()
        .text_size(px(12.))
        .text_color(theme::TEXT)
        .when(row_index % 2 == 1, |el| el.bg(gpui::rgb(0x101013)))
        .child(
            gpui::div()
                .flex_none()
                .w(px(28.))
                .text_size(px(10.))
                .child((row_index + 1).to_string()),
        )
        .child(
            gpui::div().flex_none().w(px(COL_COVER)).child(
                gpui::div()
                    .size(px(24.))
                    .rounded(px(3.))
                    .bg(theme::PANEL_RAISED),
            ),
        )
        .child(
            gpui::div()
                .flex_none()
                .w(px(COL_TITLE))
                .truncate()
                .child(item.title.clone()),
        )
        .child(gpui::div().flex_1().min_w(px(280.)))
        .child(
            gpui::div()
                .flex_none()
                .w(px(COL_ARTIST))
                .truncate()
                .child(item.artist.clone()),
        )
        .when(!compact, |el| {
            el.child(gpui::div().flex_none().w(px(COL_ALBUM)).child("—"))
        })
        .child(
            gpui::div()
                .flex_none()
                .w(px(COL_BPM))
                .text_align(gpui::TextAlign::Right)
                .child("—"),
        )
        .child(
            gpui::div()
                .flex_none()
                .w(px(COL_KEY))
                .text_align(gpui::TextAlign::Center)
                .child("—"),
        )
        .child(
            gpui::div()
                .flex_none()
                .w(px(COL_TIME))
                .text_align(gpui::TextAlign::Right)
                .child(if item.duration_ms > 0 {
                    fmt_duration(item.duration_ms as u64)
                } else {
                    "—".into()
                }),
        )
        .child(
            gpui::div()
                .id(SharedString::from(format!("import-overlay-{position}")))
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .bg(theme::with_alpha(theme::PANEL_INSET, 0.55))
                .pl(px(ROW_PAD + 28. + COL_COVER + COL_TITLE + 24.))
                .pr(px(ROW_PAD))
                .gap_3()
                .tooltip(move |_, cx| cx.new(|_| TextTip(detail.clone())).into())
                .child(
                    gpui::div()
                        .flex_1()
                        .text_size(px(11.))
                        .text_color(color)
                        .child(label),
                )
                .when(item.is_failed() && !pending && playlist.is_some(), |el| {
                    let state = state.clone();
                    el.child(
                        gpui::div()
                            .id(SharedString::from(format!("retry-import-{position}")))
                            .px_2()
                            .py_1()
                            .rounded(px(3.))
                            .bg(theme::PANEL_RAISED)
                            .text_size(px(10.))
                            .text_color(theme::TEXT)
                            .child("Retry")
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::LINE))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                state.update(cx, |s, cx| {
                                    s.retry_import_item(playlist.unwrap(), position);
                                    cx.notify();
                                });
                            }),
                    )
                })
                .when(!pending, |el| {
                    el.child(
                        gpui::div()
                            .id(SharedString::from(format!("remove-import-{position}")))
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .rounded(px(3.))
                            .bg(theme::PANEL_RAISED)
                            .text_size(px(10.))
                            .text_color(theme::TEXT)
                            .hover(|s| s.bg(theme::LINE))
                            .child("Remove")
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                if let Some(playlist) = playlist {
                                    state.update(cx, |s, cx| {
                                        s.remove_import_item(playlist, position);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                }),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_stay_between_playable_rows_including_duplicate_tracks() {
        let items: Vec<_> = ["missing", "acquired", "queued", "local", "suspect"]
            .iter()
            .enumerate()
            .map(|(position, status)| ImportItem {
                position,
                external_id: "a".into(),
                title: "Song".into(),
                artist: "Artist".into(),
                duration_ms: 1000,
                status: (*status).into(),
                track_id: matches!(*status, "acquired" | "local").then_some(TrackId(1)),
                error: None,
            })
            .collect();
        assert_eq!(
            rows(2, &items),
            vec![
                Row::Import(0),
                Row::Track(0),
                Row::Import(2),
                Row::Track(1),
                Row::Import(4)
            ]
        );
        assert_eq!(rows(0, &items[..1]), vec![Row::Import(0)]);
        assert_eq!(rows(2, &[]), vec![Row::Track(0), Row::Track(1)]);
    }
}
