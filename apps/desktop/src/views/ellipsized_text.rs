use gpui::{IntoElement, SharedString, Styled, canvas, px};

/// Truncate using the final allocated width, after flex/list measurement.
/// GPUI 0.2's TextLayout caches nowrap text without checking truncation width;
/// measuring an intrinsic width first otherwise leaves the suffix clipped out.
pub(crate) fn ellipsized_text(text: impl Into<SharedString>) -> impl IntoElement + Styled {
    let text = text.into();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, cx| {
            if bounds.size.width <= px(0.) {
                return;
            }
            let style = window.text_style();
            let font_size = style.font_size.to_pixels(window.rem_size());
            let mut runs = vec![style.to_run(text.len())];
            let display = cx
                .text_system()
                .line_wrapper(style.font(), font_size)
                .truncate_line(text, bounds.size.width, "…", &mut runs);
            let line = window
                .text_system()
                .shape_line(display, font_size, &runs, None);
            window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                let _ = line.paint(bounds.origin, bounds.size.height, window, cx);
            });
        },
    )
}
