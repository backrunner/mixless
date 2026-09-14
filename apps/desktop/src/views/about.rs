use crate::theme;
use gpui::{
    App, Bounds, Context, FocusHandle, IntoElement, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};

pub struct About {
    focus: FocusHandle,
}

impl Render for About {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if window.focused(cx).is_none() {
            self.focus.focus(window);
        }
        div()
            .id("about")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_5()
            .p_6()
            .bg(theme::PANEL)
            .text_color(theme::TEXT)
            .font_family(theme::FONT_UI)
            .on_key_down(cx.listener(|_, ev: &gpui::KeyDownEvent, window, cx| {
                let escape = ev.keystroke.key == "escape";
                let close = ev.keystroke.modifiers.platform && ev.keystroke.key == "w";
                if escape || close {
                    window.remove_window();
                    cx.stop_propagation();
                }
            }))
            .child(
                gpui::img(crate::branding::icon())
                    .size(px(88.))
                    .rounded(px(20.))
                    .border_1()
                    .border_color(theme::LINE),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("MIXLESS"),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::MUTED)
                            .child("Local music. Seamless mixing."),
                    ),
            )
            .child(div().w(px(28.)).h(px(2.)).rounded_full().bg(theme::ACCENT))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .rounded(px(theme::DIALOG_RADIUS))
                    .border_1()
                    .border_color(theme::LINE_SOFT)
                    .bg(theme::PANEL_INSET)
                    .text_size(px(11.))
                    .child(info_row(
                        "Version",
                        format!(
                            "{} · {}",
                            env!("CARGO_PKG_VERSION"),
                            env!("MIXLESS_BUILD_PROFILE")
                        ),
                        false,
                    ))
                    .child(info_row("Revision", env!("MIXLESS_REVISION").into(), false))
                    .child(info_row("Built", env!("MIXLESS_BUILD_TIME").into(), true)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::MUTED)
                    .child("MPL-2.0 · BackRunner"),
            )
    }
}

fn info_row(label: &str, value: String, last: bool) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .px_4()
        .py_2()
        .min_h(px(30.))
        .when(!last, |el| {
            el.border_b_1().border_color(theme::LINE_SOFT)
        })
        .child(div().text_color(theme::MUTED).child(label.to_string()))
        .child(div().child(value))
}

pub fn open(cx: &mut App) {
    for window in cx.windows() {
        if let Some(window) = window.downcast::<About>()
            && window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
        {
            return;
        }
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(420.), px(400.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("About Mixless".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(16.), px(18.))),
        }),
        is_resizable: false,
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |_, cx| {
        cx.new(|cx| About {
            focus: cx.focus_handle(),
        })
    }) {
        tracing::warn!("open about window: {error}");
    }
}
