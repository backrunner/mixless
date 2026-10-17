//! Portable package entry points. Export has its own dialog and selection tree.
use crate::{
    state::{PackageAction, UiState},
    theme,
};
use gpui::{AnyElement, MouseButton, Window, div, prelude::*, px};

mod menu;
#[cfg(test)]
mod tests;
mod tree;
mod widgets;

use widgets::{PackageButton, button};

impl UiState {
    pub(super) fn render_package_import(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(theme::LINE)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(11.)).child("Portable package"))
                    .child(button(
                        "package-import",
                        "Import…",
                        PackageButton::Import,
                        self.package.busy() || self.picker_open,
                        false,
                        cx,
                    )),
            )
            .when(self.package.action == Some(PackageAction::Import), |el| {
                el.child(self.render_package_status(cx))
            })
            .into_any_element()
    }

    fn render_package_status(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .when(!self.package.status.is_empty(), |el| {
                el.child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child(self.package.status.clone()),
                )
            })
            .when(self.package.busy(), |el| {
                el.child(div().flex().child(button(
                    "package-cancel",
                    "Cancel",
                    PackageButton::Cancel,
                    false,
                    false,
                    cx,
                )))
            })
            .into_any_element()
    }

    pub fn render_package_export(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let disabled = self.package.busy() || self.picker_open || !self.package.can_export();
        let panel = div()
            .id("package-export-panel")
            .debug_selector(|| "package-export-panel".into())
            .track_focus(&self.package.focus)
            .tab_group()
            .tab_index(0)
            .tab_stop(false)
            .w(px(460.).min((window.viewport_size().width - px(32.)).max(px(0.))))
            .max_h((window.viewport_size().height - px(32.)).max(px(0.)))
            .flex()
            .flex_col()
            .gap_3()
            .p_3()
            .rounded(px(theme::DIALOG_RADIUS))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .pb_2()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(div().text_size(px(13.)).child("Export package"))
                    .child(widgets::close_button(cx)),
            )
            .child(self.render_package_tree(window, cx))
            .when(!self.package.status.is_empty(), |el| {
                el.child(self.render_package_status(cx))
            })
            .child(
                div()
                    .id("package-export-footer")
                    .debug_selector(|| "package-export-footer".into())
                    .flex()
                    .flex_none()
                    .justify_end()
                    .gap_2()
                    .pt_3()
                    .border_t_1()
                    .border_color(theme::LINE)
                    .child(button(
                        "package-append",
                        "Update existing…",
                        PackageButton::Append,
                        disabled,
                        false,
                        cx,
                    ))
                    .child(button(
                        "package-export",
                        "Export…",
                        PackageButton::Export,
                        disabled,
                        true,
                        cx,
                    )),
            );
        Some(
            div()
                .id("package-export-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x000000b8))
                .on_click(cx.listener(|s, _, window, cx| {
                    s.package.export_open = false;
                    s.keyboard_focus.focus(window);
                    cx.notify();
                }))
                .child(panel)
                .into_any_element(),
        )
    }
}
