//! A complete control catalogue and clickable deck diagram share one editor.
use super::*;
use crate::views::{TextTip, ellipsized_text};
use mixless_midi::ControlKind;

mod diagram;

const MIDI_LABEL_WIDTH: f32 = 74.;
const MIDI_FIELD_GAP: f32 = 12.;
const MIDI_CONTROL_WIDTH: f32 = 194.;

impl Preferences {
    fn midi_targets(&self) -> Vec<MidiTarget> {
        MidiTarget::all()
            .into_iter()
            .filter(|t| self.midi_group.is_none_or(|g| t.group() == g))
            .collect()
    }

    pub(super) fn select_midi_target(&mut self, target: MidiTarget) {
        self.cancel_learn();
        self.midi_feedback = None;
        self.selector = None;
        self.editing = self.bindings.iter().position(|b| b.target == target);
        self.binding = self
            .editing
            .map(|i| self.bindings[i].clone())
            .unwrap_or_else(|| MidiBinding {
                device_id: self.binding.device_id.clone(),
                channel: self.binding.channel,
                number: 0,
                kind: if matches!(target.kind(), ControlKind::Button | ControlKind::Momentary) {
                    MidiSourceKind::Note
                } else {
                    MidiSourceKind::Cc
                },
                mode: MidiControlMode::Auto,
                target: target.clone(),
            });
        if self.midi_group.is_some_and(|g| g != target.group()) {
            self.midi_group = Some(target.group());
        }
        if let Some(index) = self.midi_targets().iter().position(|t| *t == target) {
            self.midi_scroll
                .scroll_to_item(index, gpui::ScrollStrategy::Top);
        }
    }

    fn quick_learn_target(&mut self, target: MidiTarget) {
        // A second click on the armed control must not discard a captured event.
        if self.quick_learning && self.binding.target == target {
            return;
        }
        self.select_midi_target(target.clone());
        let result = if self.job.is_some() {
            Err("Wait for the current settings operation to finish.".into())
        } else {
            self.core
                .midi
                .lock()
                .expect("MIDI")
                .as_ref()
                .ok_or_else(|| "MIDI is unavailable".to_string())
                .and_then(|hub| hub.learn_target(target).map_err(|e| e.to_string()))
        };
        match result {
            Ok(()) => {
                self.learning = true;
                self.quick_learning = true;
            }
            Err(error) => self.midi_feedback = Some((error, true)),
        }
    }

    pub(super) fn finish_quick_learn(&mut self, message: MidiMessage) {
        // Preserve an explicitly chosen mode when relearning the same input.
        // A new physical control starts with the target's normal default.
        let same_input = self.binding.device_id.as_ref() == Some(&message.device_id)
            && self.binding.kind == message.kind
            && self.binding.channel == message.channel
            && self.binding.number == message.number;
        if !same_input {
            self.binding.mode = MidiControlMode::Auto;
        }
        self.binding.device_id = Some(message.device_id);
        self.binding.kind = message.kind;
        self.binding.channel = message.channel;
        self.binding.number = message.number;
        let moved = self.source_conflict().map(|b| b.target.label());
        let success = format!(
            "Bound {} · {}{}",
            self.binding.target.label(),
            self.binding_text(&self.binding),
            moved.map_or(String::new(), |from| format!(" · moved from {from}")),
        );
        // Keep performance commands suppressed through the write. Clear the
        // automatic flag before polling, so a failed write cannot retry itself.
        self.quick_learning = false;
        self.save_binding();
        self.cancel_learn();
        self.midi_feedback = Some((
            if self.error {
                self.status.clone()
            } else {
                success
            },
            self.error,
        ));
    }

    fn midi_learn_bar(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let (text, color) = if self.quick_learning {
            let action = match self.binding.target.kind() {
                ControlKind::Continuous => "Move a knob or fader to bind",
                ControlKind::Encoder => "Turn the encoder to bind",
                ControlKind::Button | ControlKind::Momentary => "Press a MIDI button to bind",
            };
            (
                format!("{} · {action}…", self.binding.target.label()),
                theme::ACCENT,
            )
        } else if let Some((feedback, error)) = &self.midi_feedback {
            (
                feedback.clone(),
                if *error {
                    theme::DANGER
                } else {
                    theme::LED_GREEN
                },
            )
        } else {
            (
                "Click a control below, then move your MIDI hardware to bind it.".into(),
                theme::MUTED,
            )
        };
        let tip = text.clone();
        div()
            .flex()
            .items_center()
            .gap_2()
            .h(px(32.))
            .flex_none()
            .min_w_0()
            .px_2()
            .rounded(px(4.))
            .bg(theme::PANEL_INSET)
            .text_size(px(11.))
            .text_color(color)
            .child(
                div()
                    .id("midi-learn-message")
                    .flex_1()
                    .min_w_0()
                    .w_0()
                    .h(px(16.))
                    .child(ellipsized_text(text).w_full().h_full())
                    .tooltip(move |_, cx| cx.new(|_| TextTip(tip.clone())).into()),
            )
            .when(self.quick_learning, |el| {
                el.child(self.button(
                    "midi-quick-cancel",
                    "Cancel",
                    true,
                    |s, _, _| s.cancel_learn(),
                    cx,
                ))
            })
            .into_any_element()
    }

    fn binding_text(&self, binding: &MidiBinding) -> String {
        format!(
            "Ch {} · {:?} {}",
            binding.channel + 1,
            binding.kind,
            binding.number
        )
    }

    fn midi_select(
        &self,
        selector: Selector,
        value: String,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let width = if matches!(selector, Selector::MidiNumber) {
            58.
        } else {
            MIDI_CONTROL_WIDTH
        };
        let tip = value.clone();
        div()
            .id(SharedString::from(format!(
                "midi-select-{}",
                selector as u8
            )))
            .flex()
            .flex_none()
            .items_center()
            .gap_1()
            .w(px(width))
            .h(px(30.))
            .px_2()
            .rounded(px(4.))
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL_RAISED)
            .text_size(px(10.))
            .cursor_pointer()
            .child(ellipsized_text(value).flex_1().min_w_0().w_0().h(px(16.)))
            .child("⌄")
            .tooltip(move |_, cx| cx.new(|_| TextTip(tip.clone())).into())
            .on_click(cx.listener(move |s, _, _, cx| {
                if !s.learning && s.job.is_none() {
                    s.selector = Some(selector);
                    cx.notify();
                }
            }))
            .into_any_element()
    }

    fn source_conflict(&self) -> Option<&MidiBinding> {
        self.bindings
            .iter()
            .enumerate()
            .find(|(index, b)| {
                Some(*index) != self.editing
                    && b.device_id == self.binding.device_id
                    && b.kind == self.binding.kind
                    && b.channel == self.binding.channel
                    && b.number == self.binding.number
            })
            .map(|(_, b)| b)
    }

    fn clear_selected_mapping(&mut self) {
        let Some(index) = self.editing else {
            return;
        };
        self.cancel_learn();
        let result = self
            .core
            .midi
            .lock()
            .expect("MIDI")
            .as_ref()
            .ok_or_else(|| "MIDI is unavailable".to_string())
            .and_then(|hub| hub.remove(index).map_err(|e| e.to_string()));
        self.result(result, "Mapping cleared");
        self.poll();
    }

    pub(super) fn midi_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let busy = self.job.is_some();
        let ready = self.core.midi.lock().expect("MIDI").is_some();
        let targets = self.midi_targets();
        let bindings = self.bindings.clone();
        let selected = self.binding.target.clone();
        let state = cx.entity();
        let table = gpui::uniform_list("midi-control-list", targets.len(), move |range, _, _| {
            range
                .map(|index| {
                    let target = targets[index].clone();
                    let mapped: Vec<_> = bindings.iter().filter(|b| b.target == target).collect();
                    let address = mapped.first().map_or("Unmapped".into(), |b| {
                        let extra = if mapped.len() > 1 {
                            format!(" +{}", mapped.len() - 1)
                        } else {
                            String::new()
                        };
                        format!("Ch {} · {:?} {}{extra}", b.channel + 1, b.kind, b.number)
                    });
                    let tooltip = format!("{}\n{}", target.label(), address);
                    let active = selected == target;
                    let state = state.clone();
                    div()
                        .id(("midi-target", index))
                        .flex()
                        .items_center()
                        .gap_2()
                        .w_full()
                        .min_w_0()
                        .h(px(34.))
                        .px_2()
                        .bg(if active {
                            theme::PANEL_RAISED
                        } else {
                            theme::PANEL_INSET
                        })
                        .border_b_1()
                        .border_color(theme::LINE_SOFT)
                        .text_size(px(10.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::LINE_SOFT))
                        .child(div().size(px(5.)).flex_none().rounded_full().bg(
                            if mapped.is_empty() {
                                theme::LINE
                            } else {
                                theme::LED_GREEN
                            },
                        ))
                        .child(
                            ellipsized_text(target.label())
                                .flex_1()
                                .min_w_0()
                                .w_0()
                                .h(px(16.)),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(9.))
                                .text_color(if mapped.is_empty() {
                                    theme::MUTED
                                } else {
                                    theme::LED_GREEN
                                })
                                .child(address),
                        )
                        .tooltip(move |_, cx| cx.new(|_| TextTip(tooltip.clone())).into())
                        .on_click(move |_, _, cx| {
                            state.update(cx, |s, cx| {
                                s.select_midi_target(target.clone());
                                cx.notify();
                            })
                        })
                        .into_any_element()
                })
                .collect()
        })
        .track_scroll(self.midi_scroll.clone())
        .h(px(310.))
        .w_full()
        .min_w_0();

        div()
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(1000.))
            .mx_auto()
            .gap_3()
            .py_3()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(self.button(
                        "midi-inputs",
                        if self.midi_inputs_open {
                            "Inputs ▴"
                        } else {
                            "Inputs ▾"
                        },
                        !busy,
                        |s, _, _| s.midi_inputs_open = !s.midi_inputs_open,
                        cx,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(10.))
                            .text_color(theme::MUTED)
                            .child(format!(
                                "{} / {} controls mapped",
                                MidiTarget::all()
                                    .iter()
                                    .filter(|t| self.bindings.iter().any(|b| &b.target == *t))
                                    .count(),
                                MidiTarget::all().len()
                            )),
                    )
                    .child(self.button(
                        "import-map",
                        "Import…",
                        ready && !busy,
                        |s, _, cx| s.file_dialog(false, cx),
                        cx,
                    ))
                    .child(self.button(
                        "export-map",
                        "Export…",
                        ready && !busy,
                        |s, _, cx| s.file_dialog(true, cx),
                        cx,
                    )),
            )
            .when(self.midi_inputs_open, |el| el.child(self.midi_inputs(cx)))
            .child(self.midi_learn_bar(cx))
            .child(self.midi_diagram(cx))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        self.button(
                            "midi-all-controls",
                            "All",
                            true,
                            |s, _, _| {
                                s.midi_group = None;
                            },
                            cx,
                        )
                        .text_color(if self.midi_group.is_none() {
                            theme::ACCENT
                        } else {
                            theme::MUTED
                        }),
                    )
                    .children(MidiGroup::ALL.map(|group| {
                        self.button(
                            format!("midi-group-{group:?}"),
                            group.label(),
                            true,
                            move |s, _, _| s.midi_group = Some(group),
                            cx,
                        )
                        .px_2()
                        .text_size(px(10.))
                        .text_color(if self.midi_group == Some(group) {
                            theme::ACCENT
                        } else {
                            theme::MUTED
                        })
                    })),
            )
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap_3()
                    .min_w_0()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .border_1()
                            .border_color(theme::LINE)
                            .rounded(px(4.))
                            .overflow_hidden()
                            .child(table),
                    )
                    .child(self.midi_editor(ready && !busy, cx)),
            )
            .when_some(
                self.core.midi_error.lock().expect("MIDI error").clone(),
                |el, error| el.child(div().text_color(theme::DANGER).child(error)),
            )
            .into_any_element()
    }

    fn midi_editor(&self, enabled: bool, cx: &Context<Self>) -> gpui::AnyElement {
        let field = |label: &str, control: gpui::AnyElement| {
            div()
                .flex()
                .items_center()
                .gap(px(MIDI_FIELD_GAP))
                .child(
                    div()
                        .w(px(MIDI_LABEL_WIDTH))
                        .flex_none()
                        .text_color(theme::MUTED)
                        .child(label.to_string()),
                )
                .child(control)
        };
        let select = |selector, text| self.midi_select(selector, text, cx);
        let assigned: Vec<_> = self
            .bindings
            .iter()
            .enumerate()
            .filter(|(_, b)| b.target == self.binding.target)
            .collect();
        let conflict = self.source_conflict();
        div()
            .w(px(MIDI_LABEL_WIDTH + MIDI_FIELD_GAP + MIDI_CONTROL_WIDTH))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .text_size(px(11.))
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.binding.target.label()),
            )
            .when(assigned.len() > 1, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1()
                        .children(assigned.iter().map(|(index, b)| {
                            let index = *index;
                            self.button(
                                format!("midi-assignment-{index}"),
                                self.binding_text(b),
                                enabled,
                                move |s, _, _| {
                                    s.cancel_learn();
                                    s.binding = s.bindings[index].clone();
                                    s.editing = Some(index);
                                },
                                cx,
                            )
                            .text_size(px(9.))
                            .px_1()
                            .text_color(
                                if self.editing == Some(index) {
                                    theme::ACCENT
                                } else {
                                    theme::MUTED
                                },
                            )
                        })),
                )
            })
            .child(field(
                "Device",
                select(
                    Selector::Source,
                    self.binding
                        .device_id
                        .as_ref()
                        .map_or("Any input".into(), |id| self.port_name(id)),
                ),
            ))
            .child(field(
                "Channel",
                select(
                    Selector::MidiChannel,
                    format!("{}", self.binding.channel + 1),
                ),
            ))
            .child(field(
                "Message",
                div()
                    .flex()
                    .items_center()
                    .w(px(MIDI_CONTROL_WIDTH))
                    .flex_none()
                    .gap_1()
                    .children(
                        [(MidiSourceKind::Cc, "CC"), (MidiSourceKind::Note, "Note")].map(
                            |(kind, label)| {
                                self.button(
                                    format!("midi-kind-{label}"),
                                    label,
                                    enabled && !self.learning,
                                    move |s, _, _| {
                                        s.binding.kind = kind;
                                        if kind == MidiSourceKind::Note {
                                            s.binding.mode = MidiControlMode::Auto;
                                        }
                                    },
                                    cx,
                                )
                                .flex_1()
                                .min_w_0()
                                .px_0()
                                .text_color(
                                    if kind == self.binding.kind {
                                        theme::ACCENT
                                    } else {
                                        theme::MUTED
                                    },
                                )
                            },
                        ),
                    )
                    .child(select(
                        Selector::MidiNumber,
                        self.binding.number.to_string(),
                    ))
                    .into_any_element(),
            ))
            .when(self.binding.kind == MidiSourceKind::Cc, |el| {
                el.child(field(
                    "Mode",
                    select(Selector::MidiMode, self.binding.mode.label().into()),
                ))
            })
            .when(self.binding.target.kind() == ControlKind::Encoder, |el| {
                el.child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child("Turn right to advance; choose the encoder’s +1 / −1 values."),
                )
            })
            .when_some(conflict, |el, other| {
                el.child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::WARN)
                        .child(format!(
                            "This input controls {}. Assign will move it here.",
                            other.target.label()
                        )),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.button(
                        "midi-learn",
                        if self.learning {
                            "Cancel learn"
                        } else {
                            "Learn"
                        },
                        enabled,
                        |s, _, _| s.toggle_learn(),
                        cx,
                    ))
                    .child(
                        self.button(
                            "midi-assign",
                            "Assign",
                            enabled && !self.quick_learning,
                            |s, _, _| s.save_binding(),
                            cx,
                        )
                        .text_color(theme::ACCENT),
                    )
                    .when(self.editing.is_some(), |el| {
                        el.child(self.button(
                            "midi-clear",
                            "Clear",
                            enabled,
                            |s, _, _| s.clear_selected_mapping(),
                            cx,
                        ))
                    }),
            )
            .child(
                div()
                    .text_size(px(10.))
                    .text_color(if self.learning {
                        theme::ACCENT
                    } else {
                        theme::MUTED
                    })
                    .child(if self.learning {
                        if self
                            .core
                            .midi
                            .lock()
                            .expect("MIDI")
                            .as_ref()
                            .and_then(|h| h.learned())
                            .is_some()
                        {
                            "Signal captured — Assign to save".into()
                        } else {
                            "Move a knob or press a button…".into()
                        }
                    } else {
                        self.last.as_ref().map_or("No MIDI signal".into(), |m| {
                            format!(
                                "Last: Ch {} · {:?} {} · value {}",
                                m.channel + 1,
                                m.kind,
                                m.number,
                                m.value
                            )
                        })
                    }),
            )
            .into_any_element()
    }

    fn midi_inputs(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let busy = self.job.is_some();
        let mut panel = div()
            .flex()
            .flex_col()
            .px_3()
            .border_1()
            .border_color(theme::LINE)
            .rounded(px(4.))
            .child(row(
                "Enable MIDI",
                self.toggle(
                    "midi-enabled",
                    self.midi.enabled,
                    |s| s.midi.enabled = !s.midi.enabled,
                    cx,
                ),
            ))
            .child(row(
                "All input devices",
                self.toggle(
                    "midi-all",
                    self.midi.input_ids.is_none(),
                    |s| {
                        s.midi.input_ids = if s.midi.input_ids.is_none() {
                            Some(Vec::new())
                        } else {
                            None
                        };
                    },
                    cx,
                ),
            ));
        for port in &self.ports {
            let id = port.id.clone();
            let selected = self
                .midi
                .input_ids
                .as_ref()
                .is_none_or(|ids| ids.contains(&id));
            panel = panel.child(row(
                &port.name,
                self.button(
                    format!("midi-port-{id}"),
                    if selected { "✓" } else { "—" },
                    !busy,
                    move |s, _, _| {
                        let ids = s
                            .midi
                            .input_ids
                            .get_or_insert_with(|| s.ports.iter().map(|p| p.id.clone()).collect());
                        if ids.contains(&id) {
                            ids.retain(|p| p != &id);
                        } else {
                            ids.push(id.clone());
                        }
                    },
                    cx,
                )
                .into_any_element(),
            ));
        }
        if let Some(ids) = &self.midi.input_ids {
            for id in ids
                .iter()
                .filter(|id| !self.ports.iter().any(|p| &p.id == *id))
            {
                let id = id.clone();
                panel = panel.child(row(
                    &format!("Disconnected: {id}"),
                    self.button(
                        format!("missing-{id}"),
                        "Remove",
                        !busy,
                        move |s, _, _| {
                            if let Some(ids) = &mut s.midi.input_ids {
                                ids.retain(|p| p != &id);
                            }
                        },
                        cx,
                    )
                    .into_any_element(),
                ));
            }
        }
        panel
            .when(self.ports.is_empty(), |el| {
                el.child(info("Inputs", "No MIDI devices connected".into()))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .py_2()
                    .child(self.button(
                        "refresh-midi",
                        "Refresh devices",
                        !busy,
                        |s, _, _| s.refresh(),
                        cx,
                    ))
                    .child(
                        self.button(
                            "apply-midi",
                            "Apply inputs",
                            !busy,
                            |s, _, _| s.apply_midi(),
                            cx,
                        )
                        .text_color(theme::ACCENT),
                    ),
            )
            .into_any_element()
    }
}
