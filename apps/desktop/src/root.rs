//! Root layout and global input dispatch (drag tracking, keyboard shortcuts).

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
        let deck_min_width = (340.0 * width_scale).max(300.0);
        let vertical_wave_width = 72.0 * width_scale;
        let wave_strip_height = if compact_height { 48.0 } else { 60.0 };
        let library_height = 160.0;
        let main_min_height = if compact_height { 300.0 } else { 340.0 };

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
                // Keep the waveform flush with the content edges.  The
                // metadata column provides its own spacing; horizontal
                // padding here made every strip start with a visible blank.
                .py_1()
                .child(a)
                .child(b)
        });

        let library_key = (
            std::sync::Arc::as_ptr(&self.tracks) as usize,
            std::sync::Arc::as_ptr(&self.playlists) as usize,
            self.playlist_sel,
            self.track_sel,
            self.focus,
            self.snapshot.decks.each_ref().map(|d| d.track_id),
            self.analysis_revision,
            self.busy,
            self.picker_open,
            self.import_status(),
        );
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
        let import_modal = self.render_import_modal(window, cx);
        let fxbar = self.show_fx.then(|| self.render_fxbar(cx, mixer_width));
        let shortcuts = self.show_shortcuts.then(|| self.render_shortcuts(cx));
        let automix_bar = self.automix_active.then(|| self.render_automix_bar());
        let fx_editor = self
            .fx_editor
            .map(|(deck, slot)| self.render_fx_editor(deck, slot, cx));
        let error = self.error.clone();

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
            .on_key_down(cx.listener(|s, ev, window, cx| {
                s.handle_shortcut(ev, window, cx);
            }))
            .child(drag_capture)
            .child(topbar)
            .when_some(waves, |el, w| el.child(w))
            .when_some(automix_bar, |el, bar| el.child(bar))
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
            .when_some(import_modal, |el, modal| el.child(modal))
            .when_some(shortcuts, |el, modal| el.child(modal))
            .when_some(fx_editor, |el, modal| el.child(modal))
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
