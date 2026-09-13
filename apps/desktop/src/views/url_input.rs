use crate::{state::UiState, theme};
use gpui::{
    Bounds, MouseButton, Pixels, ShapedLine, Styled, TextRun, Window, canvas, div, point,
    prelude::*, px,
};
use std::{cell::RefCell, rc::Rc};

impl UiState {
    pub(super) fn render_url_input(
        &self,
        window: &Window,
        cx: &mut gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let focused = self.url_focus.is_focused(window);
        let text = self.url.clone();
        let empty = text.is_empty();
        let cursor = self.url_selection.cursor;
        let selection = self.url_selection.range();
        let layout: Rc<RefCell<Option<(Bounds<Pixels>, ShapedLine, Pixels)>>> =
            Rc::new(RefCell::new(None));
        let hit_layout = layout.clone();
        div()
            .id("import-url-input")
            .track_focus(&self.url_focus)
            .flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .h(px(26.))
            .px_2()
            .rounded(px(4.))
            .bg(gpui::rgb(0x0b0b0d))
            .overflow_hidden()
            .border_1()
            .border_color(if focused {
                theme::with_alpha(theme::ACCENT, 0.5)
            } else {
                theme::LINE.into()
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |s, ev: &gpui::MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    s.url_focus.focus(window);
                    let index = hit_layout
                        .borrow()
                        .as_ref()
                        .map_or(0, |(bounds, line, scroll)| {
                            line.closest_index_for_x(ev.position.x - bounds.origin.x + *scroll)
                        })
                        .min(s.url.len());
                    if ev.click_count >= 2 {
                        s.url_selection.move_to(0, false);
                        s.url_selection.move_to(s.url.len(), true);
                    } else {
                        s.url_selection.move_to(index, ev.modifiers.shift);
                    }
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|s, ev: &gpui::KeyDownEvent, window, cx| {
                s.handle_url_input_key(&ev.keystroke, window, cx);
                cx.stop_propagation();
            }))
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let display: gpui::SharedString = if empty {
                            "https://open.spotify.com/playlist/…".into()
                        } else {
                            text.clone().into()
                        };
                        let run = TextRun {
                            len: display.len(),
                            font: window.text_style().font(),
                            color: if empty {
                                gpui::rgb(0x5a5a60).into()
                            } else {
                                theme::TEXT.into()
                            },
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                        };
                        let line = window
                            .text_system()
                            .shape_line(display, px(11.), &[run], None);
                        let caret_x = if empty {
                            px(0.)
                        } else {
                            line.x_for_index(cursor)
                        };
                        let scroll = if focused {
                            (caret_x - bounds.size.width + px(2.)).max(px(0.))
                        } else {
                            px(0.)
                        };
                        let origin = point(bounds.origin.x - scroll, bounds.origin.y);
                        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                            if focused && !selection.is_empty() {
                                let left = origin.x + line.x_for_index(selection.start);
                                let width = line.x_for_index(selection.end)
                                    - line.x_for_index(selection.start);
                                crate::controls::quad_fill(
                                    window,
                                    left.into(),
                                    bounds.origin.y.into(),
                                    width.into(),
                                    16.,
                                    theme::with_alpha(theme::ACCENT, 0.3),
                                );
                            }
                            let _ = line.paint(origin, px(16.), window, cx);
                            if focused {
                                crate::controls::quad_fill(
                                    window,
                                    (origin.x + caret_x).into(),
                                    f32::from(bounds.origin.y) + 1.,
                                    1.,
                                    13.,
                                    theme::ACCENT,
                                );
                            }
                        });
                        *layout.borrow_mut() = Some((bounds, line, scroll));
                    },
                )
                .w_full()
                .h(px(16.)),
            )
    }
}
