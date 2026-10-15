//! Application shortcuts anchored below the brand, independent of window dragging.
use std::{cell::Cell, rc::Rc};

use gpui::{Bounds, IntoElement, MouseButton, Pixels, Window, div, point, prelude::*, px};

use crate::{menus, state::UiState, theme};

const ENTRIES: [(&str, &str); 4] = [
    ("Preferences...", "⌘,"),
    ("Keyboard Shortcuts", ""),
    ("Check for Updates...", ""),
    ("About mixless", ""),
];

#[derive(Default)]
pub(crate) struct BrandMenu {
    pub(crate) open: bool,
    anchor: Rc<Cell<Option<Bounds<Pixels>>>>,
    highlighted: usize,
    scroll: gpui::ScrollHandle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext, size};

    fn fixture(
        cx: &mut TestAppContext,
    ) -> (
        tempfile::TempDir,
        gpui::Entity<UiState>,
        &mut VisualTestContext,
    ) {
        let (temp, core, _) = crate::automix::tests::fixture();
        let (view, cx) = cx.add_window_view(|_, cx| UiState::new(core, cx));
        cx.simulate_resize(size(px(1180.), px(760.)));
        cx.run_until_parked();
        (temp, view, cx)
    }

    fn open(cx: &mut VisualTestContext) {
        let trigger = cx.debug_bounds("brand-window-drag").unwrap();
        cx.simulate_mouse_down(trigger.center(), MouseButton::Right, Default::default());
        cx.simulate_mouse_up(trigger.center(), MouseButton::Right, Default::default());
        cx.run_until_parked();
    }

    #[gpui::test]
    fn brand_menu_stays_anchored_and_dismisses_without_changing_controls(cx: &mut TestAppContext) {
        let (_temp, view, cx) = fixture(cx);
        open(cx);
        let trigger = cx.debug_bounds("brand-window-drag").unwrap();
        let panel = cx.debug_bounds("brand-menu-panel").unwrap();
        assert!((panel.left() - trigger.left()).abs() < px(1.));
        assert!((panel.top() - trigger.bottom() - px(5.)).abs() < px(1.));
        assert!(panel.right() <= px(1180. - 8.));
        assert!(panel.bottom() <= px(760. - 8.));
        for position in [panel.center(), trigger.center(), point(px(1150.), px(700.))] {
            cx.simulate_mouse_move(position, None, Default::default());
            cx.run_until_parked();
            assert!(view.read_with(cx, |s, _| s.brand_menu.open));
        }
        let quantize = view.read_with(cx, |s, _| s.snapshot.quantize);
        let outside = cx.debug_bounds("quantize-toggle").unwrap().center();
        cx.simulate_click(outside, Default::default());
        cx.run_until_parked();
        assert!(!view.read_with(cx, |s, _| s.brand_menu.open));
        assert_eq!(view.read_with(cx, |s, _| s.snapshot.quantize), quantize);
        open(cx);
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!view.read_with(cx, |s, _| s.brand_menu.open));
    }

    #[gpui::test]
    fn brand_menu_dispatches_settings_and_keyboard_shortcuts(cx: &mut TestAppContext) {
        let (_temp, view, cx) = fixture(cx);
        let preferences = Rc::new(Cell::new(0));
        let shortcuts = Rc::new(Cell::new(0));
        let opened = preferences.clone();
        cx.update(|_, cx| {
            cx.on_action(move |_: &menus::Preferences, _| opened.set(opened.get() + 1))
        });
        let opened = shortcuts.clone();
        cx.update(|_, cx| {
            cx.on_action(move |_: &menus::KeyboardShortcuts, _| opened.set(opened.get() + 1))
        });
        open(cx);
        let settings = cx.debug_bounds("brand-menu-entry-0").unwrap().center();
        cx.simulate_click(settings, Default::default());
        cx.run_until_parked();
        assert_eq!(preferences.get(), 1);
        assert!(!view.read_with(cx, |s, _| s.brand_menu.open));
        open(cx);
        cx.simulate_keystrokes("down enter");
        cx.run_until_parked();
        assert_eq!(shortcuts.get(), 1);
        assert!(!view.read_with(cx, |s, _| s.brand_menu.open));
    }
}

impl BrandMenu {
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
    pub(crate) fn open_brand_menu(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) {
        self.end_drag();
        self.end_momentary_fx();
        self.cancel_transport_press();
        self.track_menu = None;
        self.playlist_menu = None;
        self.deck_menu = None;
        self.audio.open = false;
        self.brand_menu.open = true;
        self.brand_menu.highlighted = 0;
        self.brand_menu.scroll = gpui::ScrollHandle::new();
        self.keyboard_focus.focus(window);
        cx.stop_propagation();
        cx.notify();
    }

    fn activate_brand_entry(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        self.brand_menu.open = false;
        let action: Box<dyn gpui::Action> = match index {
            0 => Box::new(menus::Preferences),
            1 => Box::new(menus::KeyboardShortcuts),
            2 => Box::new(menus::CheckForUpdates),
            3 => Box::new(menus::About),
            _ => return,
        };
        // Use the same handlers as the macOS menu and keyboard shortcuts.
        window.dispatch_action(action, cx);
        cx.notify();
    }

    pub(crate) fn brand_menu_key(
        &mut self,
        key: &gpui::Keystroke,
        window: &mut Window,
        cx: &mut gpui::Context<Self>,
    ) {
        match key.key.as_str() {
            "escape" | "tab" => self.brand_menu.open = false,
            "down" => {
                self.brand_menu.highlighted = (self.brand_menu.highlighted + 1) % ENTRIES.len()
            }
            "up" => {
                self.brand_menu.highlighted =
                    (self.brand_menu.highlighted + ENTRIES.len() - 1) % ENTRIES.len()
            }
            "home" => self.brand_menu.highlighted = 0,
            "end" => self.brand_menu.highlighted = ENTRIES.len() - 1,
            "enter" | "space" => self.activate_brand_entry(self.brand_menu.highlighted, window, cx),
            _ => return,
        }
        self.brand_menu
            .scroll
            .scroll_to_item(self.brand_menu.highlighted);
        cx.notify();
    }

    pub(crate) fn render_brand_menu(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        if !self.brand_menu.open {
            return None;
        }
        let anchor = self.brand_menu.anchor.get()?;
        let viewport = window.viewport_size();
        let edge = px(8.);
        let width = px(220.).min((viewport.width - edge * 2.).max(px(0.)));
        let left = anchor
            .left()
            .max(edge)
            .min((viewport.width - edge - width).max(edge));
        let top = (anchor.bottom() + px(5.)).min(viewport.height - edge);
        let height = (viewport.height - top - edge).max(px(0.));
        let mut panel = div()
            .id("brand-menu-panel")
            .debug_selector(|| "brand-menu-panel".into())
            .w(width)
            .max_h(height)
            .p(px(3.))
            .rounded(px(theme::POPUP_RADIUS))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .shadow_sm()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.brand_menu.scroll)
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation());
        for (index, (label, shortcut)) in ENTRIES.iter().enumerate() {
            let highlighted = self.brand_menu.highlighted == index;
            panel = panel.child(
                div()
                    .id(("brand-menu-entry", index))
                    .debug_selector(move || format!("brand-menu-entry-{index}"))
                    .flex_none()
                    .h(px(theme::MENU_ROW_HEIGHT))
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded(px(2.))
                    .text_size(px(11.))
                    .text_color(theme::TEXT)
                    .when(highlighted, |el| el.bg(theme::LINE))
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::LINE))
                    .on_hover(cx.listener(move |s, hovered, _, cx| {
                        if *hovered && s.brand_menu.highlighted != index {
                            s.brand_menu.highlighted = index;
                            cx.notify();
                        }
                    }))
                    .child(*label)
                    .child(div().text_color(theme::MUTED).child(*shortcut))
                    .on_click(cx.listener(move |s, _, window, cx| {
                        cx.stop_propagation();
                        s.activate_brand_entry(index, window, cx);
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
                        s.brand_menu.open = false;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|s, _, _, cx| {
                        s.brand_menu.open = false;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(gpui::anchored().position(point(left, top)).child(panel))
                .into_any_element(),
        )
    }
}
