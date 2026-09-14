//! Top bar: brand, device info, status chips, master knob. Traktor-style
//! grouped header with vertical dividers between control groups.

use gpui::prelude::*;
use gpui::{IntoElement, Rgba, Styled, px};

use crate::controls::{KnobSpec, knob};
use crate::state::{KnobCtl, UiState, WaveLayout};
use crate::theme;

fn divider() -> gpui::Div {
    gpui::div()
        .flex_none()
        .w(px(1.))
        .h(px(16.))
        .mx_1()
        .bg(theme::LINE)
}

/// LED status chip: small pill with a dot that lights up when active.
fn led_chip(label: &str, on: bool, color: Rgba) -> gpui::Div {
    gpui::div()
        .flex()
        .flex_none()
        .items_center()
        .gap_1()
        .h(px(20.))
        .px_2()
        .rounded(px(10.))
        .border_1()
        .border_color(if on {
            theme::with_alpha(color, 0.4)
        } else {
            theme::LINE.into()
        })
        .bg(if on {
            theme::with_alpha(color, 0.08)
        } else {
            theme::PANEL_INSET.into()
        })
        .child(
            gpui::div()
                .flex_none()
                .size(px(5.))
                .rounded_full()
                .bg(if on { color } else { gpui::rgb(0x3a3a40) }),
        )
        .child(
            gpui::div()
                .text_size(px(8.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(if on { theme::TEXT } else { theme::MUTED })
                .child(label.to_string()),
        )
}

impl UiState {
    pub fn render_topbar(&self, width: f32, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        // Draggable titlebar filler on either side of the centered automix
        // status; the chip itself never initiates a window drag.
        let drag_space = || {
            gpui::div()
                .flex_1()
                .h_full()
                .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                    cx.stop_propagation();
                    crate::branding::drag_main_window();
                })
        };
        let wave_top = {
            let on = self.wave_layout == WaveLayout::Top;
            let el = gpui::div()
                .id("wave-top")
                .flex()
                .items_center()
                .justify_center()
                .h(px(24.))
                .w(px(40.))
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(if on { theme::ACCENT } else { theme::MUTED })
                .bg(if on {
                    theme::with_alpha(theme::ACCENT, 0.10)
                } else {
                    theme::PANEL_INSET.into()
                })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child("TOP");
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    if let Err(error) = s.core.settings.update(|p| p.wave_layout = WaveLayout::Top)
                    {
                        s.error = error.into();
                    }
                    s.poll();
                    cx.notify();
                });
            })
        };
        let wave_center = {
            let on = self.wave_layout == WaveLayout::Center;
            let el = gpui::div()
                .id("wave-center")
                .flex()
                .items_center()
                .justify_center()
                .h(px(24.))
                .w(px(52.))
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(if on { theme::ACCENT } else { theme::MUTED })
                .bg(if on {
                    theme::with_alpha(theme::ACCENT, 0.10)
                } else {
                    theme::PANEL_INSET.into()
                })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child("CENTER");
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    if let Err(error) = s
                        .core
                        .settings
                        .update(|p| p.wave_layout = WaveLayout::Center)
                    {
                        s.error = error.into();
                    }
                    s.poll();
                    cx.notify();
                });
            })
        };
        let layout_group = gpui::div()
            .flex()
            .flex_none()
            .rounded(px(5.))
            .border_1()
            .border_color(theme::LINE)
            .overflow_hidden()
            .child(wave_top)
            .child(gpui::div().flex_none().w(px(1.)).h(px(24.)).bg(theme::LINE))
            .child(wave_center);

        let fx_btn = {
            let on = self.show_fx;
            let el = gpui::div()
                .id("fx-toggle")
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .h(px(24.))
                .px_3()
                .rounded(px(5.))
                .border_1()
                .border_color(if on {
                    theme::with_alpha(theme::WARN, 0.5)
                } else {
                    theme::LINE.into()
                })
                .bg(if on {
                    theme::with_alpha(theme::WARN, 0.12)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if on { theme::WARN } else { theme::MUTED })
                .child("FX");
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    if let Err(error) = s.core.settings.update(|p| p.show_fx = !p.show_fx) {
                        s.error = error.into();
                    }
                    s.poll();
                    cx.notify();
                });
            })
        };

        // A compact horizontal treatment keeps the control comfortably
        // inside the 44 px macOS titlebar. The percentage is easier to scan
        // than a label squeezed underneath the knob.
        let master = gpui::div()
            .id("master-control")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(32.))
            .px_2()
            .rounded(px(6.))
            .overflow_hidden()
            .border_1()
            .border_color(theme::with_alpha(theme::LED_GREEN, 0.24))
            .bg(theme::PANEL_INSET)
            // The top bar overlaps the transparent macOS titlebar.  Give the
            // whole control its own hitbox so a drag that starts beside the
            // small knob cannot fall through to the native window mover.
            .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .w(px(38.))
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        gpui::div()
                            .text_size(px(7.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::MUTED)
                            .child("MASTER"),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(10.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::LED_GREEN)
                            .child(format!(
                                "{:.0}%",
                                self.snapshot.master.clamp(0.0, 1.0) * 100.0
                            )),
                    ),
            )
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::Master,
                    value: self.snapshot.master,
                    min: 0.0,
                    max: 1.0,
                    diameter: 22.0,
                    color: theme::LED_GREEN,
                    label: "",
                    bipolar: false,
                },
                cx,
            ));

        gpui::div()
            .id("topbar")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(44.))
            .pl(px(90.))
            .pr(px(12.))
            .rounded(px(8.))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .id("brand-window-drag")
                    .child(gpui::img(crate::branding::mark()).flex_none().size(px(30.)))
                    .child(
                        gpui::div()
                            .text_size(px(11.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::TEXT)
                            .child("MIXLESS"),
                    )
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                        crate::branding::drag_main_window();
                    }),
            )
            .child(divider())
            .child(
                gpui::div()
                    .id("device-window-drag")
                    .flex()
                    .items_center()
                    .gap_2()
                    .max_w(px(if width < 1400. { 120. } else { 260. }))
                    .overflow_hidden()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(self.snapshot.device_name.clone())
                    .when(width >= 1500., |el| {
                        el.child(format!(
                            "{} Hz / {}",
                            self.snapshot.sample_rate, self.snapshot.block_frames
                        ))
                    })
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                        crate::branding::drag_main_window();
                    }),
            )
            .child(
                gpui::div()
                    .id("title-drag-space")
                    .flex()
                    .flex_1()
                    .items_center()
                    .min_w_0()
                    .h_full()
                    .child(drag_space())
                    .when(self.automix_active, |el| {
                        el.child(self.render_automix_chip(cx))
                    })
                    .child(drag_space()),
            )
            .child(
                led_chip("QUANTIZE", self.snapshot.quantize, theme::LED_GREEN)
                    .id("quantize-toggle")
                    .cursor_pointer()
                    .on_click(cx.listener(|s, _, _, cx| {
                        let on = !s.snapshot.quantize;
                        if let Err(error) = s.core.settings.update(|p| p.quantize = on) {
                            s.error = error.into();
                        } else {
                            s.dispatch(mixless_protocol::Command::SetQuantize { on });
                        }
                        cx.notify();
                    })),
            )
            .child(
                led_chip("AUTO", self.automix_active, theme::LED_GREEN)
                    .id("automix-toggle")
                    .cursor_pointer()
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.toggle_automix();
                        cx.notify();
                    })),
            )
            .child(
                led_chip(
                    if self
                        .automix_shuffle
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        "SHUFFLE ∞"
                    } else {
                        "ORDER ∞"
                    },
                    self.automix_shuffle
                        .load(std::sync::atomic::Ordering::Relaxed),
                    theme::ACCENT,
                )
                .id("automix-order")
                .cursor_pointer()
                .on_click(cx.listener(|s, _, _, cx| {
                    s.automix_shuffle
                        .fetch_xor(true, std::sync::atomic::Ordering::Relaxed);
                    cx.notify();
                })),
            )
            .when(self.automix_active, |row| {
                row.child(
                    led_chip(
                        if self.snapshot.automix_paused {
                            "RESUME"
                        } else {
                            "PAUSE"
                        },
                        self.snapshot.automix_paused,
                        theme::LED_GREEN,
                    )
                    .id("automix-pause")
                    .cursor_pointer()
                    .on_click(cx.listener(|s, _, _, cx| {
                        if !s.snapshot.automix_on {
                            return;
                        }
                        s.dispatch(if s.snapshot.automix_paused {
                            mixless_protocol::Command::ResumeAutomix
                        } else {
                            mixless_protocol::Command::PauseAutomix
                        });
                        cx.notify();
                    })),
                )
                .child(
                    led_chip("SKIP", false, theme::LED_GREEN)
                        .id("automix-skip")
                        .cursor_pointer()
                        .on_click(cx.listener(|s, _, _, cx| {
                            if !s.snapshot.automix_on {
                                return;
                            }
                            s.dispatch(mixless_protocol::Command::SkipAutomix);
                            cx.notify();
                        })),
                )
            })
            .child(divider())
            .child(layout_group)
            .child(fx_btn)
            .child(divider())
            .child(self.render_record_button(cx))
            .child(self.render_master_meter(cx))
            .child(master)
            .into_any_element()
    }
}
