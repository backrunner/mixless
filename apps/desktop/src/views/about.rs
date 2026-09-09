use crate::theme;
use gpui::{
    App, Bounds, Context, IntoElement, Render, TitlebarOptions, Window, WindowBounds,
    WindowOptions, div, prelude::*, px, size,
};

pub struct About;
impl Render for About {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .justify_center()
            .gap_3()
            .p_6()
            .bg(theme::PANEL)
            .text_color(theme::TEXT)
            .font_family(theme::FONT_UI)
            .child(
                gpui::img(crate::branding::icon())
                    .size(px(64.))
                    .rounded(px(14.)),
            )
            .child(div().text_size(px(26.)).child("Mixless"))
            .child("Local music. Seamless mixing.")
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::MUTED)
                    .child(format!(
                        "Version {} · {}",
                        env!("CARGO_PKG_VERSION"),
                        env!("MIXLESS_BUILD_PROFILE")
                    )),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::MUTED)
                    .child(format!("Revision {}", env!("MIXLESS_REVISION"))),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::MUTED)
                    .child(format!("Built {}", env!("MIXLESS_BUILD_TIME"))),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::MUTED)
                    .child("MPL-2.0 · BackRunner"),
            )
    }
}
pub fn open(cx: &mut App) {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(420.), px(380.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("About Mixless".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(16.), px(18.))),
            ..Default::default()
        }),
        is_resizable: false,
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |_, cx| cx.new(|_| About)) {
        tracing::warn!("open about window: {error}");
    }
}
