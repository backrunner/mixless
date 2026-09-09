//! Preparation stays beside the row metadata so filenames remain readable.
use crate::{analysis::Status, theme};
use gpui::{AnyElement, prelude::*, px};

pub(super) fn overlay(status: &Status) -> Option<AnyElement> {
    let color = match status {
        Status::Ready(_) => return None,
        Status::Failed(_) => theme::DANGER,
        _ => theme::MUTED,
    };
    Some(gpui::div().absolute().right(px(super::ROW_PAD)).top(px(3.))
        .px_2().h(px(20.)).rounded(px(3.)).bg(theme::PANEL_RAISED)
        .text_size(px(10.)).text_color(color).child(status.label()).into_any_element())
}
