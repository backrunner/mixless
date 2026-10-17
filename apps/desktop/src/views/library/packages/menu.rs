use crate::{state::UiState, theme};
use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*, px};

impl UiState {
    pub fn render_library_menu(
        &self,
        position: gpui::Point<gpui::Pixels>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let panel = div()
            .id("library-menu-panel")
            .debug_selector(|| "library-menu-panel".into())
            .w(px(180.).min((window.viewport_size().width - px(16.)).max(px(0.))))
            .p(px(3.))
            .rounded(px(theme::POPUP_RADIUS))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .shadow_sm()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("library-menu-export")
                    .debug_selector(|| "library-menu-export".into())
                    .h(px(theme::MENU_ROW_HEIGHT))
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .rounded(px(2.))
                    .text_size(px(11.))
                    .cursor_pointer()
                    .bg(theme::with_alpha(theme::ACCENT, 0.08))
                    .hover(|s| s.bg(theme::LINE))
                    .child("Export package…")
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.open_package_export(None);
                        cx.stop_propagation();
                        cx.notify();
                    })),
            );
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, cx| {
                        s.package.menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|s, _, _, cx| {
                        s.package.menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    gpui::anchored()
                        .position(position)
                        .snap_to_window_with_margin(px(8.))
                        .child(panel),
                )
                .into_any_element(),
        )
    }
}
