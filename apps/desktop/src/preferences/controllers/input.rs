//! Mouse-aware preset text entry using the same shaped text for painting and hit testing.
use super::*;
use gpui::{Pixels, Point, WrappedLine};
use std::{cell::RefCell, rc::Rc};

type LineLayout = (Bounds<Pixels>, WrappedLine, usize, Pixels);

fn index_at(lines: &[LineLayout], point: Point<Pixels>) -> usize {
    let line = lines.iter().min_by_key(|(bounds, _, _, _)| {
        let distance = if point.y < bounds.top() {
            bounds.top() - point.y
        } else if point.y > bounds.bottom() {
            point.y - bounds.bottom()
        } else {
            px(0.)
        };
        f32::from(distance) as u32
    });
    line.map_or(0, |(bounds, line, offset, scroll)| {
        offset
            + line
                .closest_index_for_position(
                    gpui::point(point.x - bounds.left() + *scroll, point.y - bounds.top()),
                    px(20.),
                )
                .unwrap_or_else(|index| index)
    })
}

impl Preferences {
    pub(super) fn preset_input(
        &self,
        d: &PresetDialog,
        height: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let layout: Rc<RefCell<Vec<LineLayout>>> = Rc::new(RefCell::new(Vec::new()));
        let hit = layout.clone();
        let drag = layout.clone();
        let range = d.selection.range();
        let cursor = d.selection.cursor;
        let multiline = matches!(
            d.operation,
            PresetOperation::Edit | PresetOperation::Endpoints
        );
        let wrap_width = (window.viewport_size().width - px(48.)).min(px(780.)) - px(68.);
        let mut offset = 0;
        let lines = d
            .text
            .split('\n')
            .map(|line| {
                let text: SharedString = line.to_owned().into();
                let start = offset;
                offset += line.len() + 1;
                let selection = range.start.saturating_sub(start).min(line.len())
                    ..range.end.saturating_sub(start).min(line.len());
                let caret =
                    (cursor >= start && cursor <= start + line.len()).then_some(cursor - start);
                let layout = layout.clone();
                let font = gpui::font(if multiline { "SF Mono" } else { theme::FONT_UI });
                let run = |len, selected: bool| gpui::TextRun {
                    len,
                    font: font.clone(),
                    color: theme::TEXT.into(),
                    background_color: selected.then(|| theme::with_alpha(theme::ACCENT, 0.28)),
                    underline: None,
                    strikethrough: None,
                };
                let runs = [
                    run(selection.start, false),
                    run(selection.len(), true),
                    run(text.len() - selection.end, false),
                ];
                let shaped = window
                    .text_system()
                    .shape_text(
                        text.clone(),
                        px(12.),
                        &runs,
                        multiline.then_some(wrap_width),
                        None,
                    )
                    .ok()
                    .and_then(|lines| lines.into_iter().next());
                let Some(shaped) = shaped else {
                    return div().h(px(20.)).child(text).into_any_element();
                };
                let line_height = shaped.size(px(20.)).height;
                gpui::canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let caret = caret.and_then(|i| shaped.position_for_index(i, px(20.)));
                        let scroll = if multiline {
                            px(0.)
                        } else {
                            caret.map_or(px(0.), |p| (p.x - bounds.size.width + px(3.)).max(px(0.)))
                        };
                        let origin = gpui::point(bounds.left() - scroll, bounds.top());
                        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
                            let _ = shaped.paint_background(
                                origin,
                                px(20.),
                                gpui::TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                            let _ = shaped.paint(
                                origin,
                                px(20.),
                                gpui::TextAlign::Left,
                                None,
                                window,
                                cx,
                            );
                            if let Some(caret) = caret {
                                crate::controls::quad_fill(
                                    window,
                                    (origin.x + caret.x).into(),
                                    f32::from(origin.y + caret.y) + 2.,
                                    1.,
                                    16.,
                                    theme::ACCENT,
                                );
                            }
                        });
                        layout
                            .borrow_mut()
                            .push((bounds, shaped.clone(), start, scroll));
                    },
                )
                .w_full()
                .h(line_height)
                .into_any_element()
            })
            .collect::<Vec<_>>();
        div()
            .id("preset-text")
            .debug_selector(|| "preset-text".into())
            .min_h(px(44.))
            .max_h(px((height - 340.).clamp(100., 460.)))
            .overflow_y_scroll()
            .track_scroll(&d.scroll)
            .p_3()
            .bg(theme::PANEL_INSET)
            .rounded(px(6.))
            .border_1()
            .border_color(theme::with_alpha(theme::ACCENT, 0.5))
            .font_family(if multiline { "SF Mono" } else { theme::FONT_UI })
            .cursor_text()
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |s, ev: &gpui::MouseDownEvent, _, cx| {
                    if let Some(d) = &mut s.preset_dialog {
                        let index = index_at(&hit.borrow(), ev.position).min(d.text.len());
                        if ev.click_count >= 2 {
                            d.selection.move_to(0, false);
                            d.selection.move_to(d.text.len(), true);
                        } else {
                            d.selection.move_to(index, ev.modifiers.shift);
                        }
                        d.dragging = true;
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(move |s, ev: &gpui::MouseMoveEvent, _, cx| {
                if ev.pressed_button == Some(gpui::MouseButton::Left) {
                    if let Some(d) = &mut s.preset_dialog {
                        if d.dragging {
                            let index = index_at(&drag.borrow(), ev.position).min(d.text.len());
                            d.selection.move_to(index, true);
                            cx.notify();
                        }
                    }
                }
            }))
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|s, _, _, _| {
                    if let Some(d) = &mut s.preset_dialog {
                        d.dragging = false;
                    }
                }),
            )
            .children(lines)
            .into_any_element()
    }
}
