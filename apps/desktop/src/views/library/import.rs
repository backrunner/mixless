use crate::{state::UiState, theme, views::TextTip};
use gpui::{MouseButton, Window, prelude::*, px};

impl UiState {
    pub fn render_import_modal(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> Option<gpui::AnyElement> {
        let state = cx.entity();
        let dismiss_state = state.clone();
        let close_state = state.clone();
        let spotify_state = state.clone();
        let can_import_spotify = !self.url.trim().is_empty();
        let download_dir = self.core.download_dir();
        let download_path = download_dir.display().to_string();
        let download_label = crate::state::display_dir(&download_dir);

        let close = gpui::div()
            .id("import-modal-close")
            .flex()
            .items_center()
            .justify_center()
            .size(px(26.))
            .rounded(px(4.))
            .text_size(px(16.))
            .text_color(theme::MUTED)
            .hover(|s| s.bg(theme::PANEL_RAISED).text_color(theme::TEXT))
            .child(
                gpui::svg()
                    .path("icons/close.svg")
                    .size(px(16.))
                    .flex_none()
                    .text_color(theme::MUTED),
            )
            .on_click(move |_ev, window, cx| {
                window.blur();
                close_state.update(cx, |s, cx| {
                    s.show_import_modal = false;
                    cx.notify();
                });
            });

        let spotify_button = gpui::div()
            .id("import-spotify-playlist")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .h(px(26.))
            .px_3()
            .rounded(px(3.))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .text_size(px(11.))
            .text_color(theme::TEXT)
            .when(!can_import_spotify, |el| el.opacity(0.45))
            .when(can_import_spotify, |el| {
                el.cursor_pointer().hover(|s| s.bg(theme::LINE))
            })
            .child("Import")
            .on_click(move |_ev, window, cx| {
                if can_import_spotify {
                    spotify_state.update(cx, |s, cx| {
                        if !s.url.trim().is_empty() {
                            s.import_spotify(s.url.trim().to_string());
                            s.keyboard_focus.focus(window);
                            cx.notify();
                        }
                    });
                }
            });

        let panel = gpui::div()
            .id("import-modal-panel")
            .max_h((window.viewport_size().height - px(32.)).max(px(0.)))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(460.))
            .p_3()
            .rounded(px(theme::DIALOG_RADIUS))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, _, window, cx| {
                    s.keyboard_focus.focus(window);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_click(|_ev, _window, cx| cx.stop_propagation())
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb_2()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(gpui::div().text_size(px(13.)).child("Import music"))
                    .child(close),
            )
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(gpui::div().flex_1().text_size(px(11.)).child("Local audio"))
                    .child(self.local_import_button(cx, false, "Choose files…"))
                    .child(self.local_import_button(cx, true, "Choose folder…")),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pt_3()
                    .border_t_1()
                    .border_color(theme::LINE)
                    .child(gpui::div().text_size(px(11.)).child("Spotify playlist"))
                    .child(
                        gpui::div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.render_url_input(window, cx))
                            .child(spotify_button),
                    )
                    .child(
                        gpui::div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(
                                gpui::div()
                                    .flex_none()
                                    .text_size(px(10.))
                                    .text_color(theme::MUTED)
                                    .child("Save to"),
                            )
                            .child(
                                gpui::div()
                                    .id("download-dir")
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(10.))
                                    .text_color(theme::TEXT)
                                    .child(download_label)
                                    .tooltip(move |_, cx| {
                                        cx.new(|_| TextTip(download_path.clone())).into()
                                    }),
                            )
                            .child(self.download_dir_button(cx)),
                    ),
            )
            .child(self.render_package_import(cx))
            .when(!self.import_details.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .id("import-details")
                        .max_h(px(120.))
                        .overflow_y_scroll()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .children(
                            self.import_details
                                .iter()
                                .map(|detail| gpui::div().py_1().child(detail.clone())),
                        ),
                )
            })
            .when(!self.acquire.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .pt_2()
                        .border_t_1()
                        .border_color(theme::LINE)
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child(self.import_status()),
                )
            });

        Some(
            gpui::div()
                .id("import-modal-backdrop")
                .absolute()
                .top_0()
                .right_0()
                .bottom_0()
                .left_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(gpui::rgba(0x000000b8))
                .on_click(move |_ev, window, cx| {
                    window.blur();
                    dismiss_state.update(cx, |s, cx| {
                        s.show_import_modal = false;
                        cx.notify();
                    });
                })
                .child(panel)
                .into_any_element(),
        )
    }
}
