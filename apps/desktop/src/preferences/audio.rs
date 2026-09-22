use super::*;

impl Preferences {
    pub(super) fn audio_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let snapshot = self.core.engine.snapshot();
        let settings = self.core.settings.get();
        let available = self.core.engine.audio_available();
        let busy = self.job.is_some();
        let changed = self.audio != self.core.engine.audio_config();
        let routing = setting_group(
            "Output routing",
            vec![
                row(
                    "Master output",
                    self.select(
                        Selector::Master,
                        self.audio
                            .master_device
                            .clone()
                            .unwrap_or("System Default".into()),
                        cx,
                    ),
                ),
                row(
                    "Headphones · PFL",
                    self.select(
                        Selector::Cue,
                        self.audio.cue_device.clone().unwrap_or("Disabled".into()),
                        cx,
                    ),
                ),
                row(
                    "Headphone volume",
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            self.button(
                                "cue-down",
                                "−",
                                settings.cue_gain > 0.,
                                |s, _, _| {
                                    s.save_general(|p| p.cue_gain = (p.cue_gain - 0.05).max(0.))
                                },
                                cx,
                            )
                            .w(px(30.))
                            .px_0(),
                        )
                        .child(
                            div()
                                .w(px(48.))
                                .text_center()
                                .child(format!("{:.0}%", settings.cue_gain * 100.)),
                        )
                        .child(
                            self.button(
                                "cue-up",
                                "+",
                                settings.cue_gain < 1.,
                                |s, _, _| {
                                    s.save_general(|p| p.cue_gain = (p.cue_gain + 0.05).min(1.))
                                },
                                cx,
                            )
                            .w(px(30.))
                            .px_0(),
                        )
                        .into_any_element(),
                ),
            ],
        );
        let processing = setting_group(
            "Audio engine",
            vec![
                row(
                    "Sample rate",
                    self.select(
                        Selector::Rate,
                        self.audio
                            .sample_rate
                            .map_or("Device native".into(), |rate| format!("{rate} Hz")),
                        cx,
                    ),
                ),
                row(
                    "Buffer size",
                    self.select(
                        Selector::Buffer,
                        self.audio
                            .buffer_frames
                            .map_or("Device default".into(), |frames| format!("{frames} frames")),
                        cx,
                    ),
                ),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .py_3()
                    .child(self.button(
                        "refresh-audio",
                        "Refresh devices",
                        !busy,
                        |s, _, _| s.refresh(),
                        cx,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(11.))
                            .text_color(theme::WARN)
                            .when(changed, |el| el.child("Unapplied changes")),
                    )
                    .child(self.button(
                        "revert-audio",
                        "Revert",
                        !busy && changed,
                        |s, _, _| s.audio = s.core.settings.get().audio,
                        cx,
                    ))
                    .child(
                        self.button(
                            "apply-audio",
                            "Apply audio",
                            !busy,
                            |s, _, _| s.apply_audio(),
                            cx,
                        )
                        .text_color(theme::ACCENT),
                    ),
            ],
        );
        let active = setting_group(
            "Current audio",
            vec![
                row(
                    "Status",
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(div().size(px(5.)).rounded_full().bg(if available {
                            theme::LED_GREEN
                        } else {
                            theme::DANGER
                        }))
                        .child(if available {
                            "Running"
                        } else {
                            "Audio unavailable"
                        })
                        .into_any_element(),
                ),
                info("Master", snapshot.device_name),
                info(
                    "Headphones",
                    snapshot.cue_device.unwrap_or("Disabled".into()),
                ),
                div().flex().py_3().children(
                    [
                        ("Sample rate", format!("{} Hz", snapshot.sample_rate)),
                        ("Buffer", format!("{} frames", snapshot.block_frames)),
                        (
                            "Duration",
                            format!(
                                "{:.2} ms",
                                snapshot.block_frames as f64 * 1000.
                                    / snapshot.sample_rate.max(1) as f64
                            ),
                        ),
                        ("Underruns", snapshot.xrun_count.to_string()),
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, (label, value))| {
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .when(index > 0, |el| {
                                el.pl_3().border_l_1().border_color(theme::LINE)
                            })
                            .child(
                                div()
                                    .text_size(px(10.))
                                    .text_color(theme::MUTED)
                                    .child(label),
                            )
                            .child(div().text_size(px(12.)).child(value))
                    }),
                ),
            ],
        );
        setting_stack()
            .w_full()
            .max_w(px(680.))
            .mx_auto()
            .py_5()
            .child(routing)
            .child(processing)
            .child(active)
            .into_any_element()
    }
}
