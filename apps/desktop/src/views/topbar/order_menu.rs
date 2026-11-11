//! Direct selection of playback order, with the applied mode kept visible.
use crate::{automix::OrderMode, state::UiState, theme};
use gpui::{Bounds, MouseButton, Pixels, Window, div, point, prelude::*, px};
use std::{cell::Cell, rc::Rc};
const MODES: [OrderMode; 3] = [OrderMode::Order, OrderMode::Shuffle, OrderMode::Best];

#[derive(Default)]
pub(crate) struct OrderMenu {
    pub open: bool,
    anchor: Rc<Cell<Option<Bounds<Pixels>>>>,
    highlighted: usize,
}
impl OrderMenu {
    pub(super) fn anchor_marker(&self) -> impl IntoElement {
        let anchor = self.anchor.clone();
        gpui::canvas(
            move |bounds, _, _| anchor.set(Some(bounds)),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
    }
}
impl UiState {
    pub(crate) fn open_order_menu(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.track_menu = None;
        self.playlist_menu = None;
        self.deck_menu = None;
        self.brand_menu.open = false;
        self.audio.open = false;
        self.order_menu.open = true;
        let selected = if self.automix_active {
            self.core.mix_preparation.queue_view().requested
        } else {
            self.order_button_mode()
        };
        self.order_menu.highlighted = MODES.iter().position(|m| *m == selected).unwrap_or(0);
        self.keyboard_focus.focus(window);
        cx.stop_propagation();
        cx.notify();
    }
    pub(crate) fn order_menu_key(&mut self, key: &gpui::Keystroke, cx: &mut gpui::Context<Self>) {
        match key.key.as_str() {
            "escape" | "tab" => self.order_menu.open = false,
            "up" => self.order_menu.highlighted = (self.order_menu.highlighted + 2) % 3,
            "down" => self.order_menu.highlighted = (self.order_menu.highlighted + 1) % 3,
            "home" => self.order_menu.highlighted = 0,
            "end" => self.order_menu.highlighted = 2,
            "enter" | "space" => {
                self.select_automix_order(MODES[self.order_menu.highlighted]);
                self.order_menu.open = false;
            }
            _ => return,
        }
        cx.notify();
    }
    pub(crate) fn render_order_menu(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let anchor = self.order_menu.anchor.get()?;
        let viewport = window.viewport_size();
        let width = px(190.).min(viewport.width - px(16.));
        let left = anchor
            .left()
            .clamp(px(8.), (viewport.width - width - px(8.)).max(px(8.)));
        let top = (anchor.bottom() + px(5.)).min((viewport.height - px(110.)).max(px(8.)));
        let q = &self.automix_queue;
        let selected = if self.automix_active {
            q.requested
        } else {
            self.order_button_mode()
        };
        let mut panel = div()
            .id("order-menu-panel")
            .debug_selector(|| "order-menu-panel".into())
            .w(width)
            .max_h((viewport.height - top - px(8.)).max(px(0.)))
            .p(px(3.))
            .rounded(px(theme::POPUP_RADIUS))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .shadow_sm()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation());
        for (i, mode) in MODES.into_iter().enumerate() {
            let detail = if mode == OrderMode::Best && q.error.is_some() {
                "Retry".to_string()
            } else if self.automix_active && mode == q.effective {
                "Playing".to_string()
            } else if !self.automix_active && mode == selected {
                "Selected".to_string()
            } else if mode == OrderMode::Best {
                self.order_status_text().unwrap_or_default()
            } else {
                String::new()
            };
            panel = panel.child(
                div()
                    .id(("order-menu-entry", i))
                    .debug_selector(move || format!("order-menu-entry-{i}"))
                    .h(px(theme::MENU_ROW_HEIGHT))
                    .flex_none()
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(2.))
                    .text_size(px(11.))
                    .text_color(if selected == mode {
                        theme::ACCENT
                    } else {
                        theme::TEXT
                    })
                    .when(i == self.order_menu.highlighted, |el| el.bg(theme::LINE))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::LINE))
                    .child(match mode {
                        OrderMode::Order => "Playlist order",
                        OrderMode::Shuffle => "Shuffle",
                        OrderMode::Best => "Best order",
                    })
                    .child(
                        div()
                            .text_size(px(9.))
                            .text_color(theme::MUTED)
                            .child(detail),
                    )
                    .on_hover(cx.listener(move |s, on, _, cx| {
                        if *on {
                            s.order_menu.highlighted = i;
                            cx.notify();
                        }
                    }))
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.select_automix_order(mode);
                        s.order_menu.open = false;
                        cx.stop_propagation();
                        cx.notify();
                    })),
            );
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, cx| {
                        s.order_menu.open = false;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|s, _, _, cx| {
                        s.order_menu.open = false;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(gpui::anchored().position(point(left, top)).child(panel))
                .into_any_element(),
        )
    }
}
