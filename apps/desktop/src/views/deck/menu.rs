//! The same deck actions are available from the platter and waveform lanes.
use gpui::{AnyElement, MouseButton, Pixels, Point, Window, div, prelude::*, px};
use mixless_protocol::DeckId;

use super::deck_label;
use crate::{
    state::{DeckUnloadTarget, UiState, UnloadBlock},
    theme,
};

#[derive(Clone)]
pub struct DeckMenu {
    pub target: DeckUnloadTarget,
    pub position: Point<Pixels>,
}

impl UiState {
    pub fn open_deck_menu(
        &mut self,
        deck: DeckId,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.focus = deck;
        self.track_menu = None;
        self.playlist_menu = None;
        self.audio.open = false;
        self.deck_menu = Some(DeckMenu {
            target: self.deck_unload_target(deck),
            position,
        });
        self.keyboard_focus.focus(window);
        cx.stop_propagation();
        cx.notify();
    }

    pub fn render_deck_menu(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.deck_menu.clone()?;
        let blocked: Option<UnloadBlock> = self.unload_block(menu.target);
        let enabled = blocked.is_none();
        let panel = div()
            .id("deck-menu-panel")
            .w(px(220.).min((window.viewport_size().width - px(16.)).max(px(0.))))
            .p(px(3.))
            .rounded(px(theme::POPUP_RADIUS))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .shadow_sm()
            .flex()
            .flex_col()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(7.))
                    .h(px(20.))
                    .line_height(px(20.))
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(format!("Deck {}", deck_label(menu.target.deck))),
            )
            .child(
                div()
                    .id("deck-menu-unload")
                    .h(px(theme::MENU_ROW_HEIGHT))
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .rounded(px(2.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(if enabled { theme::TEXT } else { theme::MUTED })
                    .child("Unload track")
                    .when(enabled, |el| {
                        el.cursor_pointer().hover(|s| s.bg(theme::LINE))
                    })
                    .on_click(cx.listener(move |s, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        if enabled {
                            s.unload_deck(menu.target, cx);
                        }
                    })),
            )
            .when_some(blocked, |el, reason| {
                el.child(
                    div()
                        .px(px(7.))
                        .py(px(3.))
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .whitespace_normal()
                        .child(reason.message()),
                )
            });
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, cx| {
                        s.deck_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|s, _, _, cx| {
                        s.deck_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    gpui::anchored()
                        .position(menu.position)
                        .snap_to_window_with_margin(px(8.))
                        .child(panel),
                )
                .into_any_element(),
        )
    }
}
