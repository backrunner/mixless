//! Master metering, output routing and internal recording.
use crate::{state::UiState, theme};
use gpui::{IntoElement, MouseButton, canvas, div, prelude::*, px};

impl UiState {
    pub fn render_master_meter(&self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let levels = self.snapshot.master_level;
        div()
            .id("master-output-meter")
            .flex_none()
            .w(px(108.))
            .h(px(32.))
            .px_2()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(3.))
            .rounded(px(5.))
            .bg(theme::PANEL_INSET)
            .cursor_pointer()
            .hover(|s| s.bg(theme::PANEL_RAISED))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(cx.listener(|s, _, _, cx| {
                s.toggle_audio_menu();
                cx.notify();
            }))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_size(px(7.))
                    .text_color(theme::MUTED)
                    .child("OUTPUT ▾")
                    .child("−48 · −12 · 0"),
            )
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let x = f32::from(bounds.origin.x);
                        let y = f32::from(bounds.origin.y);
                        let w = f32::from(bounds.size.width);
                        for (channel, level) in levels.iter().enumerate() {
                            let db = 20. * level.max(0.00001).log10();
                            for segment in 0..24 {
                                let threshold = -48. + segment as f32 * 2.;
                                let color = if threshold >= -2. {
                                    theme::LED_RED
                                } else if threshold >= -12. {
                                    theme::WARN
                                } else {
                                    theme::LED_GREEN
                                };
                                crate::controls::quad_fill(
                                    window,
                                    x + segment as f32 * w / 24.,
                                    y + channel as f32 * 5.,
                                    w / 24. - 1.,
                                    4.,
                                    theme::with_alpha(
                                        color,
                                        if db >= threshold { 1. } else { 0.14 },
                                    ),
                                );
                            }
                        }
                    },
                )
                .w_full()
                .h(px(9.)),
            )
            .into_any_element()
    }

    pub fn render_record_button(&self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let recording = &self.audio.recording;
        let seconds = recording.seconds as u64;
        let label = if self.audio.pending {
            "WAIT…"
        } else if recording.active {
            "■ REC"
        } else {
            "● REC"
        };
        div()
            .id("record-master")
            .flex_none()
            .h(px(26.))
            .px_2()
            .flex()
            .items_center()
            .gap(px(6.))
            .rounded(px(5.))
            .border_1()
            .border_color(theme::with_alpha(theme::LED_RED, 0.4))
            .bg(theme::with_alpha(
                theme::LED_RED,
                if recording.active { 0.18 } else { 0.04 },
            ))
            .text_size(px(9.))
            .text_color(theme::LED_RED)
            .cursor_pointer()
            .child(label)
            .when(recording.active, |el| {
                let timecode = if seconds >= 3600 {
                    format!(
                        "{:02}:{:02}:{:02}",
                        seconds / 3600,
                        seconds / 60 % 60,
                        seconds % 60
                    )
                } else {
                    format!("{:02}:{:02}", seconds / 60, seconds % 60)
                };
                el.child(
                    div()
                        .font_family("Menlo")
                        .text_size(px(10.))
                        .child(timecode),
                )
            })
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(cx.listener(|s, _, _, cx| {
                s.toggle_recording();
                cx.notify();
            }))
            .into_any_element()
    }

    pub fn render_audio_menu(&self, cx: &mut gpui::Context<Self>) -> Option<gpui::AnyElement> {
        if !self.audio.open {
            return None;
        }
        let config = self.core.engine.audio_config();
        let disabled = self.audio.pending || self.audio.recording.active;
        let mut panel = div()
            .id("audio-routing-panel")
            .absolute()
            .top(px(52.))
            .right(px(12.))
            .w(px(330.))
            .max_h(px(600.))
            .overflow_y_scroll()
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .rounded(px(8.))
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL)
            .shadow_lg()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(div().text_size(px(12.)).child("Audio output"));
        for monitor in [false, true] {
            panel = panel.child(
                div()
                    .mt_2()
                    .text_size(px(9.))
                    .text_color(theme::MUTED)
                    .child(if monitor {
                        "MONITOR · CUE / HEADPHONES"
                    } else {
                        "MASTER · SPEAKERS / RECORDING"
                    }),
            );
            let selected = if monitor {
                &config.cue_device
            } else {
                &config.master_device
            };
            let mut choices = vec![(
                None,
                if monitor {
                    "Off".to_string()
                } else {
                    format!("System default · {}", self.snapshot.device_name)
                },
            )];
            choices.extend(
                self.audio
                    .devices
                    .iter()
                    .map(|d| (Some(d.name.clone()), d.name.clone())),
            );
            for (index, (name, label)) in choices.into_iter().enumerate() {
                let selected = *selected == name;
                let conflicts = name.as_ref().is_some_and(|n| {
                    if monitor {
                        *n == self.snapshot.device_name
                    } else {
                        config.cue_device.as_ref() == Some(n)
                    }
                });
                let enabled = !disabled && !conflicts;
                panel = panel.child(
                    div()
                        .id((
                            if monitor {
                                "cue-device"
                            } else {
                                "master-device"
                            },
                            index,
                        ))
                        .px_2()
                        .py_1()
                        .rounded(px(4.))
                        .text_size(px(11.))
                        .text_color(if selected { theme::ACCENT } else { theme::TEXT })
                        .opacity(if enabled || selected { 1. } else { 0.35 })
                        .bg(if selected {
                            theme::PANEL_RAISED
                        } else {
                            theme::PANEL
                        })
                        .child(format!("{}  {}", if selected { "✓" } else { " " }, label))
                        .when(enabled, |el| {
                            el.cursor_pointer()
                                .hover(|s| s.bg(theme::PANEL_RAISED))
                                .on_click(cx.listener(move |s, _, _, cx| {
                                    s.select_audio_device(monitor, name.clone());
                                    cx.notify();
                                }))
                        }),
                );
            }
        }
        panel = panel.child(div().mt_2().text_size(px(10.)).text_color(theme::MUTED)
            .child(if self.audio.pending { "Applying audio settings…" }
                else if self.audio.recording.active { "Recording · stop REC before changing devices." }
                else if self.audio.devices.iter().all(|device| device.name == self.snapshot.device_name) {
                    "Connect a second audio output to use monitor cue."
                }
                else { "Hold CUE or a saved pad to preview on the monitor device. Release to return to cue." }));
        if let Some(path) = self.audio.recording.path.clone() {
            panel = panel.child(
                div()
                    .id("reveal-recording")
                    .mt_2()
                    .px_2()
                    .py_2()
                    .rounded(px(4.))
                    .bg(theme::PANEL_RAISED)
                    .text_size(px(10.))
                    .text_color(theme::ACCENT)
                    .cursor_pointer()
                    .child(if self.audio.recording.active {
                        "Show recording in Finder ↗"
                    } else {
                        "Show saved recording in Finder ↗"
                    })
                    .on_click(move |_, _, cx| cx.reveal_path(&path)),
            );
        } else {
            panel = panel.child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child("REC saves stereo WAV to Music / Mixless Recordings."),
            );
        }
        Some(
            div()
                .id("audio-menu-backdrop")
                .absolute()
                .inset_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|s, _, _, cx| {
                        s.audio.open = false;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }
}
