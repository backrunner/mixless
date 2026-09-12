use super::*;
use std::{cell::Cell, rc::Rc, sync::Arc, time::Instant};

#[derive(Clone)]
pub struct TrackDrag {
    pub id: TrackId,
    pub title: String,
    pub playlist: Option<i64>,
    pub index: usize,
    pub tracks: Arc<Vec<Track>>,
}

impl gpui::Render for TrackDrag {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        gpui::div()
            .flex()
            .items_center()
            .h(px(26.))
            .max_w(px(260.))
            .px(px(8.))
            .rounded(px(3.))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .text_color(theme::TEXT)
            .text_size(px(11.))
            .child(gpui::div().truncate().child(self.title.clone()))
    }
}

/// Separate upper/lower hit areas give an exact insertion point without
/// changing the row's layout or reserving a special drag handle.
pub(super) fn drop_target(
    state: gpui::Entity<UiState>,
    tracks: Arc<Vec<Track>>,
    playlist: Option<i64>,
    index: usize,
    after: bool,
) -> impl IntoElement {
    let insertion = index + usize::from(after);
    gpui::div()
        .id(("track-insertion", insertion * 2 + usize::from(after)))
        .absolute()
        .left_0()
        .right_0()
        .h(gpui::relative(0.5))
        .when(after, |el| el.bottom_0())
        .when(!after, |el| el.top_0())
        .can_drop(move |value, _, _| {
            value.downcast_ref::<TrackDrag>().is_some_and(|drag| {
                drag.playlist == playlist
                    && insertion != drag.index
                    && insertion != drag.index + 1
                    && drag
                        .tracks
                        .iter()
                        .map(|t| t.id)
                        .eq(tracks.iter().map(|t| t.id))
            })
        })
        .drag_over::<TrackDrag>(move |style, _, _, _| {
            let style = style.border_color(theme::ACCENT);
            if after {
                style.border_b_2()
            } else {
                style.border_t_2()
            }
        })
        .on_drop(move |drag: &TrackDrag, _, cx| {
            state.update(cx, |s, cx| {
                s.reorder_track(drag, insertion);
                cx.notify();
            });
        })
}

pub(super) fn start_scroll(
    scroll: gpui::UniformListScrollHandle,
    active: Rc<Cell<bool>>,
    view: gpui::WeakEntity<LibraryView>,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    if !active.replace(true) {
        scroll_frame(scroll, active, view, Instant::now(), window, cx);
    }
}

fn scroll_frame(
    scroll: gpui::UniformListScrollHandle,
    active: Rc<Cell<bool>>,
    view: gpui::WeakEntity<LibraryView>,
    last: Instant,
    window: &mut Window,
    cx: &mut gpui::App,
) {
    let handle = scroll.0.borrow().base_handle.clone();
    let bounds = handle.bounds();
    let pointer = window.mouse_position();
    if !cx.has_active_drag() || !bounds.contains(&pointer) {
        active.set(false);
        return;
    }
    let edge = 28.;
    let y = f32::from(pointer.y - bounds.top());
    let bottom = f32::from(bounds.bottom() - pointer.y);
    let velocity = if y < edge {
        (edge - y) * 12.
    } else if bottom < edge {
        -(edge - bottom) * 12.
    } else {
        active.set(false);
        return;
    };
    let now = Instant::now();
    let mut offset = handle.offset();
    offset.y += px(velocity * now.duration_since(last).as_secs_f32().min(0.05));
    handle.set_offset(offset);
    let _ = view.update(cx, |_, cx| cx.notify());
    window.on_next_frame(move |window, cx| {
        scroll_frame(scroll, active, view, now, window, cx);
    });
}
