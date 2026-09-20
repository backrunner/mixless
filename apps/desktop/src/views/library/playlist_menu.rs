use crate::{
    state::UiState,
    theme,
    views::{TextTip, ellipsized_text},
};
use gpui::{AnyElement, MouseButton, Pixels, Point, Window, div, prelude::*, px};

#[derive(Clone)]
pub struct PlaylistMenu {
    pub id: i64,
    pub name: String,
    pub position: Point<Pixels>,
}

impl UiState {
    pub fn render_playlist_menu(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.playlist_menu.clone()?;
        let failed = self
            .playlists
            .iter()
            .any(|playlist| playlist.id == menu.id && playlist.failed_imports > 0);
        let mut actions = vec![(0, "Duplicate", false)];
        if failed {
            actions.push((2, "Retry all failed tracks", false));
        }
        actions.push((1, "Remove playlist", true));
        let width = 180.;
        let height = 20. + actions.len() as f32 * (5. + theme::MENU_ROW_HEIGHT) + 8.;
        let x = f32::from(menu.position.x)
            .min(f32::from(window.viewport_size().width) - width - 8.)
            .max(8.);
        let y = f32::from(menu.position.y)
            .min(f32::from(window.viewport_size().height) - height - 8.)
            .max(8.);
        let tooltip = menu.name.clone();
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
                    .id("playlist-menu-title")
                    .w_full()
                    .min_w_0()
                    .px(px(7.))
                    .h(px(20.))
                    .line_height(px(20.))
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(ellipsized_text(menu.name.clone()).size_full())
                    .tooltip(move |_, cx| cx.new(|_| TextTip(tooltip.clone())).into()),
            );
        for (action, label, danger) in actions {
            panel = panel.child(div().h(px(1.)).my(px(2.)).mx(px(4.)).bg(theme::LINE));
            let id = menu.id;
            let name = menu.name.clone();
            panel = panel.child(
                div()
                    .id(("playlist-menu-action", action as usize))
                    .h(px(theme::MENU_ROW_HEIGHT))
                    .px(px(7.))
                    .flex()
                    .items_center()
                    .rounded(px(2.))
                    .text_size(px(11.))
                    .line_height(px(16.))
                    .text_color(if danger { theme::DANGER } else { theme::TEXT })
                    .child(label)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme::LINE))
                    .on_click(cx.listener(move |s, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        s.playlist_menu = None;
                        match action {
                            0 => s.duplicate_playlist(id),
                            2 => s.retry_failed_imports(id),
                            _ => s.confirm_remove_playlist = Some((id, name.clone())),
                        }
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
                        s.playlist_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|s, _, _, cx| {
                        s.playlist_menu = None;
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    pub fn render_remove_playlist_confirm(
        &self,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let (id, name) = self.confirm_remove_playlist.clone()?;
        let folder = self
            .playlists
            .iter()
            .find(|pl| pl.id == id)
            .is_some_and(|pl| pl.folder_path.is_some());
        let mut body =
            format!("“{name}” will be removed from the library. Tracks and audio files are kept.");
        if folder {
            body.push_str(" The folder can be re-imported later.");
        }

        let button = |id: &'static str,
                      label: &'static str,
                      bg: gpui::Rgba,
                      border: gpui::Rgba,
                      text: gpui::Rgba| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .h(px(26.))
                .px_3()
                .rounded(px(3.))
                .bg(bg)
                .border_1()
                .border_color(border)
                .text_size(px(11.))
                .text_color(text)
                .cursor_pointer()
                .child(label)
        };

        let panel = div()
            .id("remove-playlist-panel")
            .flex()
            .flex_col()
            .gap_3()
            .w(px(380.))
            .p_3()
            .rounded(px(theme::DIALOG_RADIUS))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_ev, _window, cx| cx.stop_propagation())
            .child(div().text_size(px(13.)).child("Remove playlist?"))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::MUTED)
                    .child(body),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap_2()
                    .child(
                        button(
                            "remove-playlist-cancel",
                            "Cancel",
                            theme::PANEL_RAISED,
                            theme::LINE,
                            theme::TEXT,
                        )
                        .hover(|s| s.bg(theme::LINE))
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.confirm_remove_playlist = None;
                            cx.notify();
                        })),
                    )
                    .child(
                        button(
                            "remove-playlist-confirm",
                            "Remove",
                            theme::with_alpha(theme::DANGER, 0.16).into(),
                            theme::with_alpha(theme::DANGER, 0.55).into(),
                            theme::DANGER,
                        )
                        .hover(|s| s.bg(theme::with_alpha(theme::DANGER, 0.28)))
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.confirm_remove_playlist = None;
                            s.remove_playlist(id);
                            cx.notify();
                        })),
                    ),
            );

        Some(
            div()
                .id("remove-playlist-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x000000b8))
                .on_click(cx.listener(|s, _, _, cx| {
                    s.confirm_remove_playlist = None;
                    cx.notify();
                }))
                .child(panel)
                .into_any_element(),
        )
    }
}
