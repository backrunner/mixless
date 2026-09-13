//! Preparation overlays the whole row without taking over its mouse handlers.
use crate::{analysis::Status, theme};
use gpui::{prelude::*, px, AnyElement};

pub(super) fn overlay(status: &Status) -> Option<AnyElement> {
    if matches!(status, Status::Basic(_, _) | Status::Enhancing(_)) {
        let label = match status {
            Status::Enhancing(label) => label.clone(),
            _ => "Basic analysis · Reanalyze to retry stems".into(),
        };
        return Some(
            gpui::div()
                .absolute()
                .right_2()
                .bottom_0()
                .text_size(px(9.))
                .text_color(theme::MUTED)
                .bg(theme::with_alpha(theme::PANEL_INSET, 0.8))
                .child(label)
                .into_any_element(),
        );
    }
    let (label, color) = match status {
        Status::Ready(_) => return None,
        Status::Enhancing(label) => (label.as_str(), theme::TEXT),
        Status::Basic(_, _) => ("Basic analysis · retry to enable stems", theme::MUTED),
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
