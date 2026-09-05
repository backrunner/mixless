//! Root layout and global input dispatch (drag tracking, keyboard shortcuts).

use gpui::prelude::*;
use gpui::{
    DispatchPhase, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent, Render, Styled, Window,
    canvas, px,
};
use mixless_protocol::{Command, DeckId};

use crate::state::{UiState, WaveLayout};
use crate::theme;

impl Render for UiState {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        // Register global drag capture during this canvas's paint phase.
        // Unlike hover-bound element listeners, these keep receiving events
        // after the pointer leaves a small knob/jog hitbox (notably the master
        // knob in the macOS titlebar).
        let drag_state = cx.entity();
        let release_state = cx.entity();
        let drag_capture = canvas(
            move |_, _, _| {},
            move |_, _, window, _| {
                let drag_state = drag_state.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _window, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    drag_state.update(cx, |state, _cx| {
                        if state.drag.is_some() {
                            if ev.dragging() {
                                state.drag_move(ev.position.x.into(), ev.position.y.into());
                            } else {
                                state.end_drag();
                            }
                        }
                    });
                });
                let release_state = release_state.clone();
                window.on_mouse_event(move |ev: &MouseUpEvent, phase, _window, cx| {
                    if phase != DispatchPhase::Capture || ev.button != MouseButton::Left {
                        return;
                    }
                    release_state.update(cx, |state, cx| {
                        let was_dragging = state.drag.is_some();
                        if was_dragging {
                            state.end_drag();
                        }
                        let ended_momentary = state.end_momentary_fx();
                        if was_dragging || ended_momentary {
                            cx.notify();
                        }
                    });
                });
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size(px(1.));

        let center = self.wave_layout == WaveLayout::Center;
        let viewport_width = f32::from(window.viewport_size().width);
        let viewport_height = f32::from(window.viewport_size().height);
        // Responsive fixed columns: the mixer and optional center waveforms
        // scale continuously, preserving useful deck space at the minimum window
        // size without making hardware controls too small to grab.
        let width_scale = (viewport_width / 1440.0).clamp(0.82, 1.0);
        let compact_height = viewport_height < 870.0;
        let mixer_width = 312.0 * width_scale;
        let deck_min_width = (340.0 * width_scale).max(300.0);
        let vertical_wave_width = 72.0 * width_scale;
        let wave_strip_height = if compact_height { 42.0 } else { 66.0 };
        let library_height = if compact_height { 180.0 } else { 222.0 };
        let main_min_height = if compact_height { 300.0 } else { 280.0 };

        let topbar = self.render_topbar(cx);

        let deck_a = self.render_deck(cx, DeckId::A, deck_min_width, compact_height);
        let vwave_a = center.then(|| self.render_vertical_wave(cx, DeckId::A));
        let mixer = self.render_mixer(cx, mixer_width, compact_height);
        let vwave_b = center.then(|| self.render_vertical_wave(cx, DeckId::B));
        let deck_b = self.render_deck(cx, DeckId::B, deck_min_width, compact_height);

        let waves = (!center).then(|| {
            let a = self.render_wave_strip(cx, DeckId::A, wave_strip_height);
            let b = self.render_wave_strip(cx, DeckId::B, wave_strip_height);
            gpui::div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_none()
                .p_1()
                .child(a)
                .child(b)
        });

        let library = self.render_library(cx, library_height);
        let import_modal = self.render_import_modal(window, cx);
        let fxbar = self.show_fx.then(|| self.render_fxbar(cx, mixer_width));
        let error = self.error.clone();

        gpui::div()
            .id("root")
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(theme::BG)
            .font_family(theme::FONT_UI)
            .text_color(theme::TEXT)
            .on_key_down(
                cx.listener(|s: &mut UiState, ev: &gpui::KeyDownEvent, window, cx| {
                    if s.show_import_modal && s.url_focus.is_focused(window) {
                        return;
                    }
                    let ks = &ev.keystroke;
                    if s.show_import_modal {
                        if ks.key == "escape" {
                            s.show_import_modal = false;
                            cx.notify();
                        }
                        return;
                    }
                    if ks.modifiers.control || ks.modifiers.platform || ks.modifiers.alt {
                        return;
                    }
                    let key = ks.key.as_str();
                    let cue_a = ["q", "w", "e", "r"].iter().position(|k| *k == key);
                    let cue_b = ["u", "i", "o", "p"].iter().position(|k| *k == key);
                    if key == "space" {
                        s.play_pause(s.focus);
                    } else if key == "f" {
                        s.dispatch(Command::SetCrossfader { value: 0.0 });
                    } else if key == "s" {
                        s.sync(s.focus);
                    } else if let Some(i) = cue_a {
                        s.jump_cue(DeckId::A, i);
                    } else if let Some(i) = cue_b {
                        s.jump_cue(DeckId::B, i);
                    }
                    cx.notify();
                }),
            )
            .child(drag_capture)
            .child(topbar)
            .when_some(waves, |el, w| el.child(w))
            .child(
                gpui::div()
                    .flex()
                    .flex_1()
                    .min_h(px(main_min_height))
                    .gap_2()
                    .flex_nowrap()
                    .overflow_hidden()
                    .child(deck_a)
                    .when_some(vwave_a, |el, w| {
                        el.child(
                            gpui::div()
                                .flex()
                                .flex_none()
                                .w(px(vertical_wave_width))
                                .min_h_0()
                                .py_1()
                                .child(w),
                        )
                    })
                    .child(mixer)
                    .when_some(vwave_b, |el, w| {
                        el.child(
                            gpui::div()
                                .flex()
                                .flex_none()
                                .w(px(vertical_wave_width))
                                .min_h_0()
                                .py_1()
                                .child(w),
                        )
                    })
                    .child(deck_b),
            )
            .when_some(fxbar, |el, fx| el.child(fx))
            .child(library)
            .when(!error.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .absolute()
                        .bottom_4()
                        .right_4()
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
                        .bg(gpui::rgb(0x141216))
                        .border_1()
                        .border_color(theme::with_alpha(theme::DANGER, 0.35))
                        .text_size(px(11.))
                        .text_color(gpui::rgb(0xff8a80))
                        .child(error),
                )
            })
            .when_some(import_modal, |el, modal| el.child(modal))
    }
}
