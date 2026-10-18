//! Fixed-height metadata cells keep long names inside their track row.
use super::*;

pub(super) fn cell(id: String, text: String, width: f32, color: gpui::Rgba) -> impl IntoElement {
    let tip = text.clone();
    gpui::div()
        .id(SharedString::from(id))
        .flex_none()
        .w(px(width))
        .min_w_0()
        .text_color(color)
        .tooltip(move |_, cx| cx.new(|_| TextTip(tip.clone())).into())
        .child(
            gpui::div()
                .w_full()
                .max_h(px(32.))
                .line_height(px(16.))
                .whitespace_normal()
                .line_clamp(2)
                .text_ellipsis()
                .child(text),
        )
}
