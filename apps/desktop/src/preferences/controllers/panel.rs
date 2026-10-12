//! Controller selection and responsive preset details.
use super::*;

impl Preferences {
    pub(in crate::preferences) fn controller_panel(
        &self,
        width: f32,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let selected = self.selected_profile();
        let ready = self.job.is_none();
        let exists = selected.is_some();
        let device = self
            .controller_devices
            .iter()
            .find(|d| Some(&d.id) == self.controller_device.as_ref());
        let assigned = self.hub().ok().and_then(|h| {
            h.assignments()
                .into_iter()
                .find(|a| Some(&a.device_id) == self.controller_device.as_ref())
                .and_then(|a| h.preset(&a.preset_id))
        });
        let recommended = device
            .and_then(|d| {
                self.hub().ok().map(|h| {
                    h.presets()
                        .into_iter()
                        .filter(|p| p.is_builtin() && p.matches_midi(&d.name))
                        .collect::<Vec<_>>()
                })
            })
            .filter(|p| p.len() == 1)
            .and_then(|mut p| p.pop())
            .filter(|p| Some(&p.id) != self.selected_preset.as_ref());
        let label = |text: &'static str| {
            div()
                .w(px(64.))
                .flex_none()
                .text_color(theme::MUTED)
                .child(text)
        };
        let summary = if device.is_some() {
            assigned.as_ref().map_or("No preset applied".into(), |p| {
                format!("Active preset: {}", p.name)
            })
        } else if exists {
            "Preset preview".into()
        } else {
            String::new()
        };
        let wide = width >= 800.;
        let choices = div()
            .debug_selector(|| "controller-choices".into())
            .w(px(if wide {
                (width - 24.) * 0.58
            } else {
                width.min(600.)
            }))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(label("Device"))
                    .child(
                        self.select_control(
                            Selector::ControllerDevice,
                            device.map_or("No device".into(), |d| d.name.clone()),
                            ready,
                            cx,
                        )
                        .flex_1()
                        .min_w_0()
                        .w_0(),
                    )
                    .when_some(device, |el, d| {
                        el.child(
                            div()
                                .w(px(88.))
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(theme::MUTED)
                                .child(device_status(&d.state).to_owned()),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(label("Preset"))
                    .child(
                        self.select_control(
                            Selector::MidiPreset,
                            selected
                                .as_ref()
                                .map_or("Choose a preset…".into(), |p| p.name.clone()),
                            ready,
                            cx,
                        )
                        .flex_1()
                        .min_w_0()
                        .w_0(),
                    )
                    .child(
                        self.primary_button(
                            "controller-apply",
                            "Apply",
                            ready && exists && device.is_some(),
                            |s, _, _| {
                                if let Some(id) = s.selected_preset.clone() {
                                    s.apply_midi_preset(&id, s.controller_device.clone());
                                }
                            },
                            cx,
                        )
                        .w(px(64.))
                        .px_0(),
                    )
                    .child(
                        self.select_control(Selector::PresetActions, "Manage".into(), ready, cx)
                            .w(px(88.)),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .pl(px(76.))
                    .child(
                        crate::views::ellipsized_text(summary)
                            .flex_1()
                            .min_w_0()
                            .w_0()
                            .h(px(18.))
                            .text_size(px(11.))
                            .text_color(theme::MUTED),
                    )
                    .child(
                        self.disclosure_button(
                            "controller-more",
                            "Details",
                            self.controller_advanced,
                            true,
                            |s, _, _| s.controller_advanced = !s.controller_advanced,
                            cx,
                        )
                        .w(px(88.)),
                    ),
            )
            .when_some(recommended, |el, p| {
                el.child(div().flex().pl(px(76.)).child(self.button(
                    "controller-recommendation",
                    format!("Select {}", p.name),
                    ready,
                    move |s, _, _| {
                        s.selected_preset = Some(p.id.clone());
                        s.controller_notes = false;
                    },
                    cx,
                )))
            })
            .when_some(
                device.and_then(|d| match &d.state {
                    mixless_controllers::ConnectionState::Error(error) => Some(error.clone()),
                    _ => None,
                }),
                |el, error| {
                    el.child(
                        div()
                            .id("controller-connection-error")
                            .text_size(px(11.))
                            .text_color(theme::DANGER)
                            .child("Unable to connect. Check the controller and reconnect.")
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::views::TextTip(error.clone())).into()
                            }),
                    )
                },
            )
            .when_some(
                self.controller_device.as_ref().and_then(|id| {
                    self.hub().ok().and_then(|h| {
                        h.adapter_error(id)
                            .map(|e| ("Adapter stopped. Apply the preset to restart.", e))
                            .or_else(|| {
                                h.feedback_error(id)
                                    .map(|e| ("Controller feedback unavailable.", e))
                            })
                    })
                }),
                |el, (message, error)| {
                    el.child(
                        div()
                            .id("controller-feedback-error")
                            .text_size(px(11.))
                            .text_color(theme::DANGER)
                            .child(message)
                            .tooltip(move |_, cx| {
                                cx.new(|_| crate::views::TextTip(error.clone())).into()
                            }),
                    )
                },
            );
        div()
            .debug_selector(|| "controller-panel".into())
            .flex()
            .w(px(width))
            .min_w_0()
            .items_start()
            .gap_6()
            .when(!wide, |el| el.flex_col())
            .child(choices)
            .when(self.controller_advanced, |el| {
                el.child(self.controller_details(selected, device, assigned.is_some(), wide, cx))
            })
            .into_any_element()
    }

    fn controller_details(
        &self,
        selected: Option<ControllerPreset>,
        device: Option<&mixless_controllers::ControllerDevice>,
        assigned: bool,
        wide: bool,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let ready = self.job.is_none();
        let exists = selected.is_some();
        let detail = |name: &'static str, value: String| {
            div()
                .flex()
                .gap_3()
                .text_size(px(11.))
                .child(
                    div()
                        .w(px(88.))
                        .flex_none()
                        .text_color(theme::MUTED)
                        .child(name),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("controller-detail-{name}")))
                        .flex_1()
                        .min_w_0()
                        .w_0()
                        .h(px(16.))
                        .child(
                            crate::views::ellipsized_text(value.clone())
                                .w_full()
                                .h_full(),
                        )
                        .tooltip(move |_, cx| {
                            cx.new(|_| crate::views::TextTip(value.clone())).into()
                        }),
                )
        };
        div()
            .debug_selector(|| "controller-details".into())
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .border_color(theme::LINE)
            .when(wide, |el| el.flex_1().w_0().border_l_1().pl_5())
            .when(!wide, |el| el.w_full().max_w(px(600.)).border_t_1().py_3())
            .when_some(selected, |el, p| {
                el.child(detail(
                    "Preset",
                    if p.is_builtin() {
                        "Built-in (edits save a copy)"
                    } else {
                        "User preset"
                    }
                    .into(),
                ))
                .child(detail(
                    "Mappings",
                    p.adapter.as_ref().map_or_else(
                        || p.bindings.len().to_string(),
                        |a| format!("{} controls (adapter)", a.controls.len()),
                    ),
                ))
                .when_some(p.community.as_ref(), |el, c| {
                    el.child(detail("Author", c.author.clone()))
                        .child(detail("Version", c.package_version.clone()))
                        .child(detail("License", c.license.clone()))
                })
                .child(detail(
                    "Feedback",
                    if p.adapter.is_some() {
                        "Adapter".into()
                    } else {
                        p.feedback.len().to_string()
                    },
                ))
                .child(detail(
                    "Validation",
                    match p.verification {
                        mixless_controllers::Verification::Protocol => "Not hardware-tested",
                        mixless_controllers::Verification::Simulation => "Simulation tested",
                        mixless_controllers::Verification::Hardware => "Hardware tested",
                    }
                    .into(),
                ))
                .when(!p.notes.is_empty(), |el| {
                    el.child(div().flex().child(self.disclosure_button(
                        "controller-notes",
                        "Preset notes",
                        self.controller_notes,
                        true,
                        |s, _, _| s.controller_notes = !s.controller_notes,
                        cx,
                    )))
                    .when(self.controller_notes, |el| {
                        el.child(
                            div()
                                .id("controller-notes-text")
                                .debug_selector(|| "controller-notes-text".into())
                                .max_h(px(120.))
                                .overflow_y_scroll()
                                .text_size(px(11.))
                                .line_height(px(17.))
                                .text_color(theme::MUTED)
                                .child(p.notes),
                        )
                    })
                })
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.button(
                        "endpoints",
                        "Ports…",
                        ready && exists && device.is_some(),
                        |s, _, _| s.begin_preset_dialog(PresetOperation::Endpoints),
                        cx,
                    ))
                    .when(device.is_some_and(|d| d.hid), |el| {
                        el.child(self.button(
                            "add-hid-field",
                            "Add HID field",
                            ready
                                && exists
                                && !self.selected_profile().is_some_and(|p| p.adapter.is_some()),
                            |s, _, _| s.add_hid_field(),
                            cx,
                        ))
                        .child(self.button(
                            "read-hid-feature",
                            "Read feature…",
                            ready,
                            |s, _, _| s.begin_preset_dialog(PresetOperation::Feature),
                            cx,
                        ))
                    })
                    .when(self.recommended_audio().is_some(), |el| {
                        el.child(self.button(
                            "controller-audio",
                            "Audio routing…",
                            ready,
                            |s, _, _| s.apply_recommended_audio(),
                            cx,
                        ))
                    })
                    .when(device.is_some(), |el| {
                        el.child(self.disclosure_button(
                            "controller-monitor",
                            "Input monitor",
                            self.controller_monitor,
                            true,
                            |s, _, _| s.controller_monitor = !s.controller_monitor,
                            cx,
                        ))
                    })
                    .child(self.button(
                        "controller-unassign",
                        "Unassign",
                        ready && assigned,
                        |s, _, _| {
                            let result = s.hub().and_then(|h| {
                                h.unassign(s.controller_device.as_deref().unwrap_or_default())
                                    .map_err(|e| e.to_string())
                            });
                            s.result(result, "Device unassigned");
                            s.poll();
                        },
                        cx,
                    )),
            )
            .when(self.controller_monitor, |el| {
                let packet = self
                    .hub()
                    .ok()
                    .and_then(|h| h.last_packet())
                    .filter(|p| Some(&p.device_id) == self.controller_device.as_ref());
                el.child(
                    div()
                        .debug_selector(|| "controller-monitor-content".into())
                        .flex()
                        .flex_col()
                        .gap_2()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .when_some(device, |el, d| el.child(format!("Device ID: {}", d.id)))
                        .child(div().font_family("SF Mono").child(packet.map_or(
                            "Waiting for input…".into(),
                            |p| {
                                p.data
                                    .iter()
                                    .take(48)
                                    .map(|b| format!("{b:02X}"))
                                    .collect::<Vec<_>>()
                                    .join(" ")
                            },
                        ))),
                )
            })
            .into_any_element()
    }
}
