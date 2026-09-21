//! Preparation status and source-file recovery actions.
use crate::{analysis::Status, state::UiState, theme};
use gpui::{AnyElement, prelude::*, px};
use mixless_protocol::TrackId;

pub(super) fn overlay(
    status: &Status,
    track: TrackId,
    state: gpui::Entity<UiState>,
) -> Option<AnyElement> {
    if let Status::MissingFile(path) = status {
        let path = path.clone();
        return Some(
            gpui::div()
                .absolute()
                .right_0()
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .justify_end()
                .gap_2()
                .px_3()
                .bg(theme::with_alpha(theme::PANEL_INSET, 0.86))
                .child(
                    gpui::div()
                        .id("missing-file-label")
                        .text_size(px(11.))
                        .text_color(theme::DANGER)
                        .child("File not found")
                        .tooltip(move |_, cx| {
                            cx.new(|_| crate::views::TextTip(path.clone().into()))
                                .into()
                        }),
                )
                .children([(0, "Locate file…"), (1, "Retry"), (2, "Remove")].map(
                    |(action, label)| {
                        let state = state.clone();
                        let tip = match action {
                            0 => "Choose the local audio file for this track.",
                            1 => "Search known track folders again.",
                            _ => "Remove from the library and all playlists; keep the audio file.",
                        };
                        gpui::div()
                            .id(("missing-file-action", action as usize))
                            .px_2()
                            .py_1()
                            .rounded(px(3.))
                            .bg(theme::PANEL_RAISED)
                            .text_size(px(11.))
                            .text_color(if action == 2 {
                                theme::DANGER
                            } else {
                                theme::TEXT
                            })
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::LINE))
                            .child(label)
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::views::TextTip(tip.into())).into()
                            })
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .on_click(move |_, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                state.update(cx, |s, cx| {
                                    match action {
                                        0 => s.choose_track_file(track, cx),
                                        1 => s.retry_track_file(track),
                                        _ => s.remove_library_track(track),
                                    }
                                    cx.notify();
                                });
                            })
                    },
                ))
                .into_any_element(),
        );
    }
    let (label, color) = match status {
        Status::Ready(_) => return None,
        Status::Failed(_) => ("Analysis failed", theme::DANGER),
        Status::Queued => ("Queued", theme::MUTED),
        Status::Checking => ("Checking file", theme::TEXT),
        _ => ("Analyzing", theme::TEXT),
    };
    Some(
        gpui::div()
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme::with_alpha(theme::PANEL_INSET, 0.78))
            .child(
                gpui::div()
                    .id("analysis-status-label")
                    .px_3()
                    .py_1()
                    .rounded(px(4.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(color)
                    .child(label.to_string())
                    .when_some(
                        if let Status::Failed(error) = status {
                            Some(error.clone())
                        } else {
                            None
                        },
                        |el, error| {
                            el.tooltip(move |_, cx| {
                                cx.new(|_| crate::views::TextTip(error.clone().into()))
                                    .into()
                            })
                        },
                    ),
            )
            .into_any_element(),
    )
}

/// Separation progress is supplementary; only basic analysis covers the row.
pub(super) fn stem_badge(status: Option<&crate::analysis::StemStatus>) -> Option<AnyElement> {
    use crate::analysis::StemStatus;
    let label = match status? {
        StemStatus::Queued => "Stems · Queued".to_owned(),
        StemStatus::Running(label) => label.clone(),
        StemStatus::Ready => return None,
        StemStatus::Failed(_) => "Stems unavailable".to_owned(),
    };
    Some(
        gpui::div()
            .absolute()
            .right_2()
            .bottom_0()
            .text_size(px(9.))
            .text_color(theme::MUTED)
            .bg(theme::with_alpha(theme::PANEL_INSET, 0.8))
            .child(label)
            .into_any_element(),
    )
}
