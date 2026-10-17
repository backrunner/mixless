//! Small continuous arc, static when macOS Reduce Motion is enabled.
use crate::theme;
use gpui::{Animation, AnimationExt, IntoElement, div, prelude::*, px};
use std::time::Duration;

pub(super) fn indicator() -> gpui::AnyElement {
    let indicator = div()
        .id("automix-loading")
        .debug_selector(|| "automix-loading".into())
        .flex_none()
        .size(px(12.));
    if crate::motion::reduce_motion() {
        return indicator.child(arc(0.)).into_any_element();
    }
    indicator
        .with_animation(
            "automix-loading-rotation",
            Animation::new(Duration::from_millis(1000)).repeat(),
            |el, phase| el.child(arc(phase)),
        )
        .into_any_element()
}

fn arc(phase: f32) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let mut path = gpui::PathBuilder::stroke(px(1.5));
            for step in 0..=24 {
                let angle = std::f32::consts::TAU * (phase + step as f32 / 24. * 0.7);
                let position =
                    bounds.center() + gpui::point(px(angle.cos() * 4.5), px(angle.sin() * 4.5));
                if step == 0 {
                    path.move_to(position);
                } else {
                    path.line_to(position);
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, theme::ACCENT);
            }
        },
    )
    .size_full()
}
