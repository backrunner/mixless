//! Preparation overlays the whole row without taking over its mouse handlers.
use crate::{analysis::Status, theme};
use gpui::{AnyElement, prelude::*, px};

pub(super) fn overlay(status: &Status) -> Option<AnyElement> {
    let (label, color) = match status {
        Status::Ready(_) => return None,
        Status::Failed(_) => ("Analysis failed", theme::DANGER),
        Status::Queued => ("Queued", theme::MUTED),
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
                    .px_3()
                    .py_1()
                    .rounded(px(4.))
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(color)
                    .child(label.to_string()),
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
