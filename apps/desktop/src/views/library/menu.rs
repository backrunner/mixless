use crate::{state::UiState, theme};
use gpui::{AnyElement, MouseButton, Pixels, Point, Window, div, prelude::*, px};
use mixless_protocol::TrackId;

#[derive(Clone)]
pub struct TrackMenu {
    pub track: TrackId,
    pub playlist: Option<i64>,
    pub title: String,
    pub position: Point<Pixels>,
}

impl UiState {
    pub fn render_track_menu(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.track_menu.clone()?;
        let width = 224.;
        let height = 20. + 2. * 5. + 3. * theme::MENU_ROW_HEIGHT + 8.;
        let x = f32::from(menu.position.x)
            .min(f32::from(window.viewport_size().width) - width - 8.)
            .max(8.);
        let y = f32::from(menu.position.y)
            .min(f32::from(window.viewport_size().height) - height - 8.)
            .max(8.);
        let mut panel = div()
            .absolute()
            .left(px(x))
            .top(px(y))
            .w(px(width))
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
                    .truncate()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(menu.title),
            );
        for (action, label, enabled) in [
            (0, "Remove from playlist", menu.playlist.is_some()),
            (1, "Reanalyze", true),
            (2, "Reset cues & reanalyze", true),
        ] {
            if action < 2 {
                panel = panel.child(div().h(px(1.)).my(px(2.)).mx(px(4.)).bg(theme::LINE));
            }
            let track = menu.track;
            let playlist = menu.playlist;
            panel = panel.child(
                div()
                    .id(("track-menu-action", action as usize))
                    .h(px(theme::MENU_ROW_HEIGHT))
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .rounded(px(2.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(if !enabled {
                        theme::MUTED
                    } else if action == 2 {
                        theme::DANGER
                    } else {
                        theme::TEXT
                    })
                    .child(div().flex_1().child(label))
                    .when(action == 1, |el| {
                        el.child(
                            div()
                                .text_size(px(10.))
                                .text_color(theme::MUTED)
                                .child("Keep cues"),
                        )
                    })
                    .when(action == 1, |el| {
                        el.tooltip(|_, cx| {
                            cx.new(|_| {
                                super::PathTip("Reanalyze audio; keep manually placed cues.".into())
                            })
                            .into()
                        })
                    })
                    .when(enabled, |el| {
                        el.cursor_pointer().hover(|s| s.bg(theme::LINE))
                    })
                    .on_click(cx.listener(move |s, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        if enabled {
                            s.track_menu = None;
                            if action == 0 {
                                s.remove_playlist_track(playlist.unwrap(), track);
                            } else {
                                s.reanalyze_track(track, action == 2);
                            }
                            cx.notify();
                        }
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
                        s.track_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|s, _, _, cx| {
                        s.track_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }
}
