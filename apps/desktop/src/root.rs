//! Root layout and global input dispatch (drag tracking, keyboard shortcuts).

pub(crate) mod overlays;

use gpui::prelude::*;
use gpui::{
    DispatchPhase, IntoElement, MouseButton, MouseMoveEvent, MouseUpEvent, Render, Styled, Window,
    canvas, px,
};
use mixless_protocol::DeckId;

use crate::state::{UiState, WaveLayout};
use crate::theme;

impl Render for UiState {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let draw_started = crate::performance::begin();
        let active_decks = self.snapshot.decks.iter().filter(|d| d.playing).count();
        if let Some((x, y)) = self.pending_drag.take() {
            self.drag_move(x, y);
        }
        self.poll();
        if window.focused(cx).is_none()
            || (!self.show_import_modal && self.url_focus.is_focused(window))
        {
            self.keyboard_focus.focus(window);
        }
        if self.package.export_open && !self.package.focus.contains_focused(window, cx) {
            self.package.focus.focus(window);
        }
        if self.needs_continuous_repaint() {
            window.request_animation_frame();
        }
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
                                state.pending_drag =
                                    Some((ev.position.x.into(), ev.position.y.into()));
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
                            state.drag_move(ev.position.x.into(), ev.position.y.into());
                            state.end_drag();
                        }
                        let ended_momentary = state.end_momentary_fx();
                        let ended_transport = state.end_transport_press();
                        if was_dragging || ended_momentary || ended_transport {
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
        let mixer_width = 288.0;
        // +50px floor for the per-deck channel strip beside the jog.
        let deck_min_width = (340.0 * width_scale).max(350.0);
        let vertical_wave_width = 72.0 * width_scale;
        let wave_strip_height = if compact_height { 48.0 } else { 60.0 };
        let library_height = 160.0;
        let main_min_height = if compact_height { 300.0 } else { 340.0 };

        let topbar = self.render_topbar(window.viewport_size().width.into(), cx);

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
                // Keep the waveform flush with the content edges.  The
                // metadata column provides its own spacing; horizontal
                // padding here made every strip start with a visible blank.
                .py_1()
                .child(a)
                .child(b)
        });

        let library_key = self.library_render_key(viewport_width, window.scale_factor());
        if self.library_key.as_ref() != Some(&library_key) {
            self.library_key = Some(library_key);
            self.library_view.update(cx, |_, cx| cx.notify());
        }
        let library = gpui::AnyView::from(self.library_view.clone()).cached(
            gpui::div()
                .flex_1()
                .w_full()
                .min_h(px(library_height))
                .style()
                .clone(),
        );
        let fxbar = self.show_fx.then(|| self.render_fxbar(cx, mixer_width));
        let (popups, closing_popup) = self.render_popups(window, cx);
        let error = self.error.clone();
        let update_notice = self.update_notice.clone();

        gpui::div()
            .id("root")
            .track_focus(&self.keyboard_focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(theme::BG)
            .font_family(theme::FONT_UI)
            .text_color(theme::TEXT)
            .capture_key_down(move |_, window, cx| {
                if closing_popup {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            })
            .capture_key_up(move |_, window, cx| {
                if closing_popup {
                    window.prevent_default();
                    cx.stop_propagation();
                }
            })
            .on_key_down(cx.listener(|s, ev: &gpui::KeyDownEvent, window, cx| {
                if s.package.export_open {
                    if !s.picker_open {
                        match ev.keystroke.key.as_str() {
                            "escape" => {
                                s.package.export_open = false;
                                s.keyboard_focus.focus(window);
                                cx.notify();
                            }
                            "tab" => {
                                if ev.keystroke.modifiers.shift {
                                    window.focus_prev();
                                } else {
                                    window.focus_next();
                                }
                            }
                            _ => {}
                        }
                    }
                    cx.stop_propagation();
                    return;
                }
                if s.package.menu.is_some() {
                    match ev.keystroke.key.as_str() {
                        "escape" | "tab" => s.package.menu = None,
                        "enter" | "space" => s.open_package_export(None),
                        _ => {}
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if s.master_mixer.open {
                    if ev.keystroke.key == "escape" {
                        s.close_master_mixer();
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                if s.brand_menu.open {
                    s.brand_menu_key(&ev.keystroke, window, cx);
                    cx.stop_propagation();
                    return;
                }
                if ev.keystroke.key == "escape" && cx.stop_active_drag(window) {
                    cx.stop_propagation();
                    return;
                }
                if s.audio.open {
                    if ev.keystroke.key == "escape" {
                        s.audio.open = false;
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                if s.deck_menu.is_some() {
                    match ev.keystroke.key.as_str() {
                        "escape" => s.deck_menu = None,
                        "enter" => {
                            let target = s.deck_menu.as_ref().unwrap().target;
                            if s.unload_block(target).is_none() {
                                s.unload_deck(target, cx);
                            }
                        }
                        _ => {}
                    }
                    cx.notify();
                    cx.stop_propagation();
                    return;
                }
                if s.track_menu.is_some() {
                    if ev.keystroke.key == "escape" {
                        s.track_menu = None;
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                if s.playlist_menu.is_some() {
                    if ev.keystroke.key == "escape" {
                        s.playlist_menu = None;
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                if s.confirm_remove_playlist.is_some() {
                    if ev.keystroke.key == "escape" {
                        s.confirm_remove_playlist = None;
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                s.handle_shortcut(ev, window, cx);
            }))
            .child(drag_capture)
            .child(topbar)
            .when_some(waves, |el, w| el.child(w))
            .child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .h(px(main_min_height))
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
            .when(!update_notice.is_empty(), |el| {
                el.child(
                    gpui::div()
                        .absolute()
                        .bottom_4()
                        .left_4()
                        .px_3()
                        .py_2()
                        .rounded(px(6.))
                        .bg(gpui::rgb(0x141216))
                        .border_1()
                        .border_color(theme::with_alpha(theme::ACCENT, 0.35))
                        .text_size(px(11.))
                        .text_color(theme::ACCENT)
                        .child(update_notice),
                )
            })
            .children(popups)
            .when(draw_started.is_some(), |el| {
                el.child(
                    canvas(
                        |_, _, _| {},
                        move |_, _, _, _| crate::performance::finish(draw_started, active_decks),
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size(px(1.)),
                )
            })
    }
}
