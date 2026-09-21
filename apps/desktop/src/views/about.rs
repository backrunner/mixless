use crate::theme;
use gpui::{
    App, Bounds, ClipboardItem, Context, FocusHandle, IntoElement, Render, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};

const WEBSITE: &str = "https://mixless.alkinum.com";
const GUIDE: &str = "https://mixless.alkinum.com/docs";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const LICENSE: &str = "https://github.com/backrunner/mixless/blob/main/LICENSE";

pub struct About {
    focus: FocusHandle,
    copied: bool,
}

impl About {
    fn copy_build_info(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(format!(
            "mixless {}\nChannel: {}\nProfile: {}\nRevision: {}\nBuilt: {}",
            env!("CARGO_PKG_VERSION"),
            env!("MIXLESS_CHANNEL"),
            env!("MIXLESS_BUILD_PROFILE"),
            env!("MIXLESS_REVISION"),
            env!("MIXLESS_BUILD_TIME"),
        )));
        self.copied = true;
        cx.notify();
    }
}

impl Render for About {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if window.focused(cx).is_none() {
            self.focus.focus(window);
        }
        let channel = match env!("MIXLESS_CHANNEL") {
            "stable" => "Stable",
            "beta" => "Beta",
            _ => "Development",
        };
        div()
            .id("about")
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .pt(px(46.))
            .pb(px(22.))
            .px(px(28.))
            .bg(theme::PANEL)
            .text_color(theme::TEXT)
            .font_family(theme::FONT_UI)
            .on_key_down(cx.listener(|s, ev: &gpui::KeyDownEvent, window, cx| {
                let modifiers = &ev.keystroke.modifiers;
                if ev.keystroke.key == "escape" || modifiers.platform && ev.keystroke.key == "w" {
                    window.remove_window();
                    cx.stop_propagation();
                } else if ev.keystroke.key == "tab" {
                    if modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    cx.stop_propagation();
                } else if modifiers.platform && ev.keystroke.key == "c" {
                    s.copy_build_info(cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(20.))
                    .child(gpui::img(crate::branding::icon()).size(px(80.)).flex_none())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(7.))
                            .child(
                                div()
                                    .text_size(px(24.))
                                    .line_height(px(29.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .child(crate::branding::wordmark()),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .line_height(px(17.))
                                    .text_color(theme::TEXT)
                                    .child(format!("Version {}", env!("CARGO_PKG_VERSION"))),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .text_size(px(10.))
                                    .line_height(px(14.))
                                    .text_color(theme::MUTED)
                                    .child(div().size(px(4.)).rounded_full().bg(theme::ACCENT))
                                    .child(channel),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .mt(px(26.))
                    .mb(px(26.))
                    .child(link("about-website", "Website", WEBSITE))
                    .child(link("about-guide", "User guide", GUIDE))
                    .child(link("about-github", "GitHub", REPOSITORY)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .border_t_1()
                    .border_color(theme::LINE)
                    .pt(px(16.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .mb(px(10.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Build information"),
                            )
                            .child(
                                div()
                                    .id("about-copy")
                                    .focusable()
                                    .tab_stop(true)
                                    .cursor_pointer()
                                    .px(px(9.))
                                    .py(px(4.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(theme::LINE)
                                    .text_size(px(10.))
                                    .text_color(if self.copied {
                                        theme::LED_GREEN
                                    } else {
                                        theme::MUTED
                                    })
                                    .hover(|s| s.bg(theme::PANEL_RAISED).text_color(theme::TEXT))
                                    .focus(|s| s.border_color(theme::ACCENT))
                                    .child(if self.copied { "Copied" } else { "Copy" })
                                    .tooltip(|_, cx| {
                                        cx.new(|_| {
                                            super::TextTip(
                                                "Copy version and build information (⌘C)".into(),
                                            )
                                        })
                                        .into()
                                    })
                                    .on_click(cx.listener(|s, _, _, cx| s.copy_build_info(cx)))
                                    .on_key_down(cx.listener(
                                        |s, ev: &gpui::KeyDownEvent, _, cx| {
                                            if ev.keystroke.key == "enter"
                                                || ev.keystroke.key == "space"
                                            {
                                                s.copy_build_info(cx);
                                                cx.stop_propagation();
                                            }
                                        },
                                    )),
                            ),
                    )
                    .child(info_row("Profile", env!("MIXLESS_BUILD_PROFILE")))
                    .child(info_row("Revision", env!("MIXLESS_REVISION")))
                    .child(info_row("Built", env!("MIXLESS_BUILD_TIME"))),
            )
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .border_t_1()
                    .border_color(theme::LINE_SOFT)
                    .pt(px(16.))
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child("© 2026 BackRunner")
                    .child(
                        div()
                            .id("about-license")
                            .focusable()
                            .tab_stop(true)
                            .cursor_pointer()
                            .border_1()
                            .border_color(gpui::transparent_black())
                            .rounded(px(3.))
                            .px(px(4.))
                            .py(px(2.))
                            .hover(|s| s.text_color(theme::TEXT))
                            .focus(|s| s.border_color(theme::ACCENT))
                            .child("MPL 2.0 ↗")
                            .on_click(|_, _, cx| cx.open_url(LICENSE))
                            .on_key_down(|ev, _, cx| activate_link(ev, LICENSE, cx)),
                    ),
            )
    }
}

fn link(id: &'static str, label: &'static str, url: &'static str) -> impl IntoElement {
    div()
        .id(id)
        .focusable()
        .tab_stop(true)
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .gap(px(7.))
        .h(px(32.))
        .rounded(px(5.))
        .border_1()
        .border_color(theme::LINE)
        .bg(theme::PANEL_RAISED)
        .text_size(px(11.))
        .cursor_pointer()
        .hover(|s| s.border_color(theme::MUTED))
        .focus(|s| s.border_color(theme::ACCENT))
        .child(label)
        .child(div().text_color(theme::MUTED).child("↗"))
        .tooltip(move |_, cx| cx.new(|_| super::TextTip(url.into())).into())
        .on_click(move |_, _, cx| cx.open_url(url))
        .on_key_down(move |ev, _, cx| activate_link(ev, url, cx))
}

fn activate_link(ev: &gpui::KeyDownEvent, url: &'static str, cx: &mut App) {
    if !ev.is_held && (ev.keystroke.key == "enter" || ev.keystroke.key == "space") {
        cx.open_url(url);
        cx.stop_propagation();
    }
}

fn info_row(label: &'static str, value: &'static str) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(16.))
        .h(px(24.))
        .text_size(px(11.))
        .text_color(theme::MUTED)
        .child(label)
        .child(div().text_color(theme::TEXT).child(value))
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
            size(px(420.), px(424.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("About mixless".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(16.), px(18.))),
        }),
        is_resizable: false,
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |_, cx| {
        cx.new(|cx| About {
            focus: cx.focus_handle().tab_stop(false),
            copied: false,
        })
    }) {
        tracing::warn!("open about window: {error}");
    }
}
