use super::*;

pub(super) fn save_audio_settings(
    core: &AppCore,
    config: AudioConfig,
    restore: Option<crate::settings::ControllerAudioRestore>,
) -> Result<(), String> {
    let _apply = core.settings_apply.lock().expect("apply settings");
    save_audio_settings_with(
        &core.settings,
        core.engine.audio_config(),
        config,
        restore,
        |config| {
            core.engine
                .configure_audio(config)
                .map_err(|e| e.to_string())
        },
    )
}

// The transaction is also exercised with a simulated device, without opening
// physical outputs. Production always uses Engine::configure_audio above.
pub(super) fn save_audio_settings_with(
    settings: &crate::settings::SettingsStore,
    previous: AudioConfig,
    config: AudioConfig,
    restore: Option<crate::settings::ControllerAudioRestore>,
    mut configure: impl FnMut(AudioConfig) -> Result<(), String>,
) -> Result<(), String> {
    configure(config.clone())?;
    if let Err(error) = settings.update(|s| {
        s.audio = config;
        s.controller_audio_restore = restore;
    }) {
        return match configure(previous) {
            Ok(()) => Err(error),
            Err(rollback) => Err(format!("{error}; audio restore failed: {rollback}")),
        };
    }
    Ok(())
}

impl Preferences {
    pub(super) fn save_headphone_mix(&mut self, value: f32) {
        let result = self
            .core
            .settings
            .update(|p| p.cue_mix = value)
            .and_then(|()| {
                self.core
                    .engine
                    .dispatch(Command::SetCueMix { value })
                    .map_err(|e| e.to_string())
            });
        self.result(result, "Headphone mix saved");
    }
    pub(super) fn audio_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let snapshot = self.core.engine.snapshot();
        let settings = self.core.settings.get();
        let available = self.core.engine.audio_available();
        let busy = self.job.is_some();
        let changed = self.audio != self.core.engine.audio_config();
        let routing = setting_group(
            "Outputs",
            vec![
                row(
                    "Master output",
                    self.select(
                        Selector::Master,
                        self.audio
                            .master_device
                            .clone()
                            .unwrap_or("System default".into()),
                        cx,
                    ),
                ),
                row(
                    "Headphones",
                    self.select(
                        Selector::Cue,
                        self.audio.cue_device.clone().unwrap_or("Disabled".into()),
                        cx,
                    ),
                ),
                row(
                    "Master channels",
                    self.select(
                        Selector::MasterChannels,
                        format!(
                            "{} / {}",
                            self.audio.master_channels[0] + 1,
                            self.audio.master_channels[1] + 1
                        ),
                        cx,
                    ),
                ),
                row(
                    "Headphone channels",
                    self.select(
                        Selector::CueChannels,
                        format!(
                            "{} / {}",
                            self.audio.cue_channels[0] + 1,
                            self.audio.cue_channels[1] + 1
                        ),
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
                row(
                    "Headphone mix",
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(
                            self.button(
                                "cue-mix-down",
                                "−",
                                snapshot.cue_mix > 0.,
                                |s, _, _| {
                                    let value = (s.core.engine.snapshot().cue_mix - 0.05).max(0.);
                                    s.save_headphone_mix(value)
                                },
                                cx,
                            )
                            .w(px(30.))
                            .px_0(),
                        )
                        .child(
                            div()
                                .w(px(92.))
                                .text_center()
                                .child(format!("{:.0}% Master", snapshot.cue_mix * 100.)),
                        )
                        .child(
                            self.button(
                                "cue-mix-up",
                                "+",
                                snapshot.cue_mix < 1.,
                                |s, _, _| {
                                    let value = (s.core.engine.snapshot().cue_mix + 0.05).min(1.);
                                    s.save_headphone_mix(value)
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
            "Format",
            vec![
                row(
                    "Sample rate",
                    self.select(
                        Selector::Rate,
                        self.audio
                            .sample_rate
                            .map_or("Device default".into(), |rate| format!("{rate} Hz")),
                        cx,
                    ),
                ),
                row(
                    "Bit depth",
                    self.select(Selector::Depth, self.audio.bit_depth.map_or("Device default".into(), |bits| format!("{bits}-bit")), cx),
                ),
                div().px_3().py_2().text_size(px(11.)).line_height(px(16.)).text_color(theme::MUTED).child("Applies to the master output. A separate headphone device keeps its own format. Playback briefly stops when applying changes."),
                row(
                    "Buffer size",
                    self.select(
                        Selector::Buffer,
                        self.audio
                            .buffer_frames
                            .map_or(AudioConfig::automatic_buffer_label().into(), |frames| {
                                format!("{frames} frames")
                            }),
                        cx,
                    ),
                ),
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3()
                    .py_2()
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
                        self.primary_button(
                            "apply-audio",
                            "Apply",
                            !busy,
                            |s, _, _| s.apply_audio(),
                            cx,
                        ),
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
                    "Master format",
                    snapshot
                        .master_output
                        .as_ref()
                        .map_or("Unavailable".into(), |output| {
                            format!(
                                "{} Hz, {}",
                                output.sample_rate,
                                output
                                    .hardware_format
                                    .as_deref()
                                    .unwrap_or(&output.stream_format)
                            )
                        }),
                ),
                info(
                    "Processing stream",
                    snapshot
                        .master_output
                        .as_ref()
                        .map_or("Unavailable".into(), |output| output.stream_format.clone()),
                ),
                info(
                    "Headphones",
                    snapshot.cue_device.unwrap_or("Disabled".into()),
                ),
                info(
                    "Headphone format",
                    snapshot
                        .cue_output
                        .as_ref()
                        .map_or("Disabled".into(), |output| {
                            format!(
                                "{} Hz, {}",
                                output.sample_rate,
                                output
                                    .hardware_format
                                    .as_deref()
                                    .unwrap_or(&output.stream_format)
                            )
                        }),
                ),
                div().flex().px_3().py_2().children(
                    [
                        ("Sample rate", format!("{} Hz", snapshot.sample_rate)),
                        ("Buffer", format!("{} frames", snapshot.block_frames)),
                        (
                            "Buffer duration",
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
            .child(routing)
            .child(processing)
            .child(
                self.button(
                    "audio-details",
                    if self.audio_details_open {
                        "Hide device details"
                    } else {
                        "Device details…"
                    },
                    true,
                    |s, _, _| s.audio_details_open = !s.audio_details_open,
                    cx,
                )
                .bg(theme::BG)
                .border_0()
                .justify_start()
                .px_0(),
            )
            .when(self.audio_details_open, |el| el.child(active))
            .into_any_element()
    }
}
