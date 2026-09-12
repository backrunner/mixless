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
                    .child(label),
            )
            .into_any_element(),
    )
}
