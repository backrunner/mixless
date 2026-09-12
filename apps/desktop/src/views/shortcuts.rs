use gpui::{Context, IntoElement, MouseButton, div, prelude::*, px};

use crate::{shortcuts::HELP, state::UiState, theme};

impl UiState {
    pub fn render_shortcuts(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let panel = div()
            .id("shortcuts-panel")
            .w(px(600.))
            .p_3()
            .rounded(px(theme::DIALOG_RADIUS))
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .flex()
            .flex_col()
            .gap_2()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div().flex().items_center().justify_between().pb_2().border_b_1().border_color(theme::LINE)
                    .child(div().text_size(px(13.)).child("Keyboard shortcuts"))
                    .child(div().id("shortcuts-close").px_2().cursor_pointer()
                        .text_color(theme::MUTED).child("×")
                        .on_click(cx.listener(|s, _, _, cx| {
                            s.show_shortcuts = false;
                            cx.notify();
                        }))),
            )
            .child(div().text_size(px(11.)).text_color(theme::MUTED)
                .child(format!("Selected deck: {:?} · highlighted on the console", self.focus)))
            .children(HELP.iter().map(|(keys, description)| {
                div().flex().items_center().gap_3().min_h(px(theme::MENU_ROW_HEIGHT)).text_size(px(11.))
                    .child(div().w(px(195.)).flex_none().text_color(theme::ACCENT).child(*keys))
                    .child(div().flex_1().child(*description))
            }))
            .child(div().text_size(px(11.)).text_color(theme::MUTED)
                .child("Shortcuts are paused in dialogs. Hold does not repeat. Cues are saved with the track."));

        div()
            .id("shortcuts-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x000000b8))
            .on_click(cx.listener(|s, _, _, cx| {
                s.show_shortcuts = false;
                cx.notify();
            }))
            .child(panel)
            .into_any_element()
    }
}
