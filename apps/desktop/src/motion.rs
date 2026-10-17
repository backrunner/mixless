//! Shared popup presence. Closing content stays mounted until its fade completes.
use gpui::{AnyElement, IntoElement, MouseButton, Window, div, prelude::*};
use std::time::{Duration, Instant};

const ENTER: Duration = Duration::from_millis(160);
const EXIT: Duration = Duration::from_millis(120);

pub(crate) struct Presence<T> {
    value: Option<T>,
    open: bool,
    from: f32,
    started: Option<Instant>,
}

impl<T> Default for Presence<T> {
    fn default() -> Self {
        Self {
            value: None,
            open: false,
            from: 0.,
            started: None,
        }
    }
}

impl<T: Clone> Presence<T> {
    pub fn sample(&mut self, target: Option<T>, now: Instant, reduced: bool) -> Option<Frame<T>> {
        let open = target.is_some();
        if open != self.open {
            self.from = self.opacity(now);
            self.started = Some(now);
            self.open = open;
        }
        if target.is_some() {
            self.value = target;
        }
        if reduced {
            self.started = None;
        }
        let opacity = self.opacity(now);
        let destination = if self.open { 1. } else { 0. };
        let animating = opacity != destination;
        if !animating {
            self.started = None;
            if !self.open {
                self.value = None;
            }
        }
        self.value.as_ref().map(|value| Frame {
            value: value.clone(),
            opacity,
            closing: !self.open,
            animating,
        })
    }

    fn opacity(&self, now: Instant) -> f32 {
        let destination = if self.open { 1. } else { 0. };
        let Some(started) = self.started else {
            return destination;
        };
        let duration = if self.open { ENTER } else { EXIT };
        let phase = (now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32())
            .clamp(0., 1.);
        // Ease out without moving controls or changing the popup's layout.
        let eased = 1. - (1. - phase).powi(3);
        self.from + (destination - self.from) * eased
    }
}

pub(crate) struct Frame<T> {
    pub value: T,
    pub opacity: f32,
    pub closing: bool,
    animating: bool,
}

pub(crate) struct Popup {
    pub element: AnyElement,
    pub closing: bool,
}

impl<T> Frame<T> {
    pub fn render(
        self,
        id: &'static str,
        window: &mut Window,
        render: impl FnOnce(T, &mut Window) -> Option<AnyElement>,
    ) -> Option<Popup> {
        if self.animating {
            window.request_animation_frame();
        }
        let content = render(self.value, window)?;
        let element = div()
            .id(id)
            .absolute()
            .inset_0()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .opacity(self.opacity)
                    .child(content),
            )
            // A fading popup is visible but no longer actionable. The shield
            // also prevents its final click/scroll from reaching the workspace.
            .when(self.closing, |el| el.child(input_shield()))
            .into_any_element();
        Some(Popup {
            element,
            closing: self.closing,
        })
    }
}

fn input_shield() -> impl IntoElement {
    div()
        .id("popup-exit-shield")
        .absolute()
        .inset_0()
        .occlude()
        .on_mouse_down(MouseButton::Left, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Right, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Middle, |_, window, cx| {
            window.prevent_default();
            cx.stop_propagation();
        })
        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_mouse_up(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
}

/// Outgoing popups sit below newly opened ones, irrespective of popup type.
pub(crate) fn stack(mut popups: Vec<Popup>) -> (Vec<AnyElement>, bool) {
    let closing_only = !popups.is_empty() && popups.iter().all(|p| p.closing);
    popups.sort_by_key(|p| !p.closing);
    (
        popups.into_iter().map(|p| p.element).collect(),
        closing_only,
    )
}

pub(crate) fn reduce_motion() -> bool {
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn mixless_reduce_motion() -> i32;
        }
        // Read the system preference on the UI thread; never change it.
        unsafe { mixless_reduce_motion() != 0 }
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(test)]
mod tests;
