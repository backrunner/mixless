use super::*;
use crate::state::url_input::UrlSelection;
mod audio_routing;
mod import;
mod import_batch;
pub(super) use audio_routing::{ControllerAudioDraft, PendingAudioRestore};
mod input;
mod names;
mod panel;
pub(super) use names::{natural_cmp, preset_label};

use mixless_controllers::{ControllerPreset, DeviceAssignment, DeviceMatcher, Endpoint};

#[derive(Clone, Copy)]
pub(super) enum PresetOperation {
    New,
    Rename,
    Duplicate,
    Edit,
    Endpoints,
    Delete,
    Feature,
}
#[derive(Clone)]
pub(super) struct PresetDialog {
    operation: PresetOperation,
    text: String,
    selection: UrlSelection,
    ports: String,
    scroll: gpui::ScrollHandle,
    dragging: bool,
}
pub(super) fn device_status(state: &mixless_controllers::ConnectionState) -> &str {
    use mixless_controllers::ConnectionState::*;
    match state {
        Discovered => "Available",
        Connected => "Connected",
        Offline => "Disconnected",
        Error(_) => "Connection failed",
    }
}

impl Preferences {
    pub(super) fn hub(&self) -> Result<Arc<mixless_controllers::ControllerManager>, String> {
        self.core
            .midi
            .lock()
            .expect("controllers")
            .clone()
            .ok_or("Controllers unavailable".into())
    }
    pub(super) fn selected_profile(&self) -> Option<ControllerPreset> {
        self.hub().ok()?.preset(self.selected_preset.as_deref()?)
    }
    fn begin_preset_dialog(&mut self, operation: PresetOperation) {
        self.cancel_learn();
        self.selector = None;
        self.error = false;
        self.status.clear();
        let text = match operation {
            PresetOperation::New => "My controller".into(),
            PresetOperation::Rename => self.selected_profile().map_or(String::new(), |p| p.name),
            PresetOperation::Duplicate => self
                .selected_profile()
                .map_or("My controller".into(), |p| format!("{} — Copy", p.name)),
            PresetOperation::Delete => String::new(),
            PresetOperation::Feature => "0 64".into(),
            PresetOperation::Edit => self
                .selected_profile()
                .and_then(|p| serde_json::to_string_pretty(&p).ok())
                .unwrap_or_default(),
            PresetOperation::Endpoints => {
                let mut a = self
                    .hub()
                    .ok()
                    .and_then(|h| {
                        h.assignments()
                            .into_iter()
                            .find(|a| Some(&a.device_id) == self.controller_device.as_ref())
                    })
                    .unwrap_or(DeviceAssignment {
                        device_id: self.controller_device.clone().unwrap_or_default(),
                        preset_id: self.selected_preset.clone().unwrap_or_default(),
                        ports: Default::default(),
                        outputs: Default::default(),
                    });
                a.preset_id = self.selected_preset.clone().unwrap_or_default();
                if let Some(p) = self.selected_profile() {
                    for e in p.endpoints {
                        a.ports
                            .entry(e.id)
                            .or_insert_with(|| self.controller_device.clone().unwrap_or_default());
                    }
                }
                serde_json::to_string_pretty(&a).unwrap_or_default()
            }
        };
        let mut selection = UrlSelection::default();
        selection.move_to(text.len(), true);
        let ports = if matches!(operation, PresetOperation::Endpoints) {
            self.controller_devices
                .iter()
                .map(|d| format!("Input {}: {}", d.name, d.id))
                .chain(
                    mixless_controllers::output_ports()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|d| format!("Output {}: {}", d.name, d.id)),
                )
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        self.preset_dialog = Some(PresetDialog {
            ports,
            operation,
            text,
            selection,
            scroll: gpui::ScrollHandle::new(),
            dragging: false,
        });
    }
    pub(super) fn preset_key(&mut self, key: &gpui::Keystroke, cx: &mut Context<Self>) {
        let Some(d) = self.preset_dialog.as_mut() else {
            return;
        };
        if key.key == "escape" {
            self.preset_dialog = None;
            return;
        }
        if key.modifiers.platform {
            match key.key.as_str() {
                "a" => {
                    d.selection.move_to(0, false);
                    d.selection.move_to(d.text.len(), true);
                }
                "c" => cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                    d.text[d.selection.range()].to_string(),
                )),
                "v" => {
                    if let Some(text) = cx.read_from_clipboard().and_then(|v| v.text()) {
                        if text.len() + d.text.len() < 4 * 1024 * 1024 {
                            d.selection.replace(&mut d.text, &text);
                        }
                    }
                }
                "x" => {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                        d.text[d.selection.range()].to_string(),
                    ));
                    d.selection.replace(&mut d.text, "");
                }
                "enter" => {
                    self.finish_preset_dialog();
                }
                _ => {}
            }
            return;
        }
        match key.key.as_str() {
            "enter"
                if matches!(
                    d.operation,
                    PresetOperation::Edit | PresetOperation::Endpoints
                ) =>
            {
                d.selection.replace(&mut d.text, "\n")
            }
            "enter" => self.finish_preset_dialog(),
            "backspace" | "delete" => {
                if d.selection.range().is_empty() {
                    let next = d.selection.adjacent(&d.text, key.key == "delete");
                    d.selection.move_to(next, true);
                }
                d.selection.replace(&mut d.text, "");
            }
            "left" | "right" => {
                let next = d.selection.adjacent(&d.text, key.key == "right");
                d.selection.move_to(next, key.modifiers.shift);
            }
            "up" | "down" => {
                let before = &d.text[..d.selection.cursor];
                let line_start = before.rfind('\n').map_or(0, |i| i + 1);
                let column = d.text[line_start..d.selection.cursor].chars().count();
                let start = if key.key == "up" {
                    d.text[..line_start.saturating_sub(1)]
                        .rfind('\n')
                        .map_or(0, |i| i + 1)
                } else {
                    d.text[d.selection.cursor..]
                        .find('\n')
                        .map_or(d.text.len(), |i| d.selection.cursor + i + 1)
                };
                let line = d.text[start..].split('\n').next().unwrap_or_default();
                let index = line
                    .char_indices()
                    .nth(column)
                    .map_or(line.len(), |(i, _)| i);
                d.selection.move_to(start + index, key.modifiers.shift);
            }
            "home" => {
                let start = d.text[..d.selection.cursor]
                    .rfind('\n')
                    .map_or(0, |i| i + 1);
                d.selection.move_to(start, key.modifiers.shift);
            }
            "end" => {
                let end = d.text[d.selection.cursor..]
                    .find('\n')
                    .map_or(d.text.len(), |i| d.selection.cursor + i);
                d.selection.move_to(end, key.modifiers.shift);
            }
            _ => {
                if !key.modifiers.control && !key.modifiers.alt {
                    if let Some(ch) = &key.key_char {
                        d.selection.replace(&mut d.text, ch);
                    }
                }
            }
        }
        if let Some(d) = &self.preset_dialog {
            d.scroll.scroll_to_item(
                d.text[..d.selection.cursor]
                    .bytes()
                    .filter(|b| *b == b'\n')
                    .count(),
            );
        }
    }
    fn finish_preset_dialog(&mut self) {
        let Some(d) = self.preset_dialog.as_ref() else {
            return;
        };
        let operation = d.operation;
        let text = d.text.clone();
        let id = self.selected_preset.clone().unwrap_or_default();
        if matches!(operation, PresetOperation::Feature) {
            let values: Vec<_> = text.split_whitespace().collect();
            let parsed = values
                .first()
                .and_then(|s| s.parse::<u8>().ok())
                .zip(values.get(1).and_then(|s| s.parse::<usize>().ok()));
            if let (Some((report, length)), Some(device), Ok(hub)) =
                (parsed, self.controller_device.clone(), self.hub())
            {
                self.preset_dialog = None;
                self.start_job(move || {
                    JobResult::File(
                        hub.read_hid_feature(&device, report, length)
                            .map(|data| {
                                format!(
                                    "Feature {report}: {}",
                                    data.iter()
                                        .map(|v| format!("{v:02X}"))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                )
                            })
                            .map_err(|e| e.to_string()),
                    )
                });
            } else {
                self.result(
                    Err("Enter report ID and report length as decimal numbers".into()),
                    "",
                );
            }
            return;
        }
        let result = self.hub().and_then(|hub| match operation {
            PresetOperation::Feature => unreachable!(),
            PresetOperation::New => {
                let mut p = ControllerPreset::blank(&text);
                if let Some(device) = self
                    .controller_devices
                    .iter()
                    .find(|d| Some(&d.id) == self.controller_device.as_ref() && d.hid)
                {
                    p.endpoints = vec![Endpoint {
                        id: "main".into(),
                        matcher: DeviceMatcher::Hid {
                            vendor_id: device.vendor_id.unwrap(),
                            product_id: device.product_id.unwrap(),
                            interface: device.interface,
                            usage_page: device.usage_page,
                            usage: device.usage,
                        },
                    }];
                }
                hub.save_preset(p).map(Some).map_err(|e| e.to_string())
            }
            PresetOperation::Rename => hub
                .rename_preset(&id, &text)
                .map(|_| Some(id.clone()))
                .map_err(|e| e.to_string()),
            PresetOperation::Duplicate => hub
                .duplicate_preset(&id, &text)
                .map(Some)
                .map_err(|e| e.to_string()),
            PresetOperation::Edit => serde_json::from_str::<ControllerPreset>(&text)
                .map_err(|e| e.to_string())
                .and_then(|mut p| {
                    p.id = id.clone();
                    hub.save_device_preset(p, self.controller_device.as_deref())
                        .map(Some)
                        .map_err(|e| e.to_string())
                }),
            PresetOperation::Endpoints => serde_json::from_str::<DeviceAssignment>(&text)
                .map_err(|e| e.to_string())
                .and_then(|a| {
                    hub.assign(a)
                        .map(|_| Some(id.clone()))
                        .map_err(|e| e.to_string())
                }),
            PresetOperation::Delete => hub
                .delete_preset(&id)
                .map(|_| None)
                .map_err(|e| e.to_string()),
        });
        match result {
            Ok(id) => {
                self.selected_preset = id;
                self.preset_dialog = None;
                self.result(Ok(()), "Preset saved");
                self.poll();
            }
            Err(e) => self.result(Err(e), ""),
        }
    }
    fn add_hid_field(&mut self) {
        let Some(mut p) = self.selected_profile() else {
            return;
        };
        let Some(endpoint) = p
            .endpoints
            .iter()
            .find(|e| matches!(e.matcher, DeviceMatcher::Hid { .. }))
            .map(|e| e.id.clone())
        else {
            self.result(Err("Select a HID preset first".into()), "");
            return;
        };
        let report = self
            .hub()
            .ok()
            .and_then(|h| h.last_packet())
            .filter(|p| p.hid && Some(&p.device_id) == self.controller_device.as_ref())
            .and_then(|p| p.data.first().copied())
            .unwrap_or(0);
        let mut b = mixless_controllers::Binding::from_legacy(&self.binding);
        b.id = mixless_controllers::new_id();
        b.endpoint = endpoint;
        b.source = mixless_controllers::InputSource::Hid {
            report_id: report,
            bit_offset: 8,
            bits: 8,
            signed: false,
            little_endian: true,
        };
        b.mode = mixless_controllers::ValueMode::Absolute;
        p.bindings.push(b);
        self.begin_preset_dialog(PresetOperation::Edit);
        if let Some(d) = self.preset_dialog.as_mut() {
            d.text = serde_json::to_string_pretty(&p).unwrap_or_default();
            d.selection.move_to(d.text.len(), false);
        }
    }
    pub(super) fn preset_actions(&self) -> Vec<(String, String)> {
        let preset = self.selected_profile();
        let mut actions = vec![("new", "New preset…"), ("import", "Import…")];
        if let Some(p) = preset {
            actions.push(("duplicate", "Duplicate…"));
            if p.adapter.is_none() {
                actions.push(("edit", "Edit MIDI / HID…"));
            }
            actions.push(("export", "Export…"));
            if !p.is_builtin() {
                actions.extend([("rename", "Rename…"), ("delete", "Delete…")]);
            }
        }
        actions
            .into_iter()
            .map(|(value, label)| (value.into(), label.into()))
            .collect()
    }

    pub(super) fn run_preset_action(&mut self, action: &str, cx: &mut Context<Self>) {
        if self.job.is_some()
            || !self
                .preset_actions()
                .iter()
                .any(|(value, _)| value == action)
        {
            return;
        }
        match action {
            "new" => self.begin_preset_dialog(PresetOperation::New),
            "duplicate" => self.begin_preset_dialog(PresetOperation::Duplicate),
            "rename" => self.begin_preset_dialog(PresetOperation::Rename),
            "edit" => self.begin_preset_dialog(PresetOperation::Edit),
            "delete" => self.begin_preset_dialog(PresetOperation::Delete),
            "import" => self.file_dialog(false, cx),
            "export" => self.file_dialog(true, cx),
            _ => {}
        }
    }

    fn accept_preset_import(&mut self) {
        if self.import_batch.is_some() {
            self.accept_preset_batch();
            return;
        }
        if let Some(report) = self.import_preview.clone() {
            match self.hub().and_then(|h| {
                h.accept_import(Arc::unwrap_or_clone(report))
                    .map_err(|e| e.to_string())
            }) {
                Ok(id) => {
                    self.selected_preset = Some(id);
                    self.import_preview = None;
                    self.result(Ok(()), "Preset imported; select a device to apply it");
                }
                Err(e) => self.result(Err(e), ""),
            }
        }
    }

    pub(super) fn controller_dialog(
        &self,
        dialog: &overlays::ControllerPopup,
        height: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        if let overlays::ControllerPopup::Import { report, from_batch } = dialog {
            return Self::controller_dialog_overlay(self.preset_import_panel(
                report,
                *from_batch,
                height,
                window,
                cx,
            ));
        }
        if let overlays::ControllerPopup::Batch(batch) = dialog {
            return Self::controller_dialog_overlay(
                self.preset_batch_panel(batch, height, window, cx),
            );
        }
        let mut panel = div()
            .id("controller-dialog-panel")
            .debug_selector(|| "controller-dialog-panel".into())
            .flex()
            .flex_col()
            .w_full()
            .max_w(px(780.))
            .overflow_y_scroll()
            .gap_3()
            .p_5()
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .rounded(px(6.))
            .shadow_lg()
            .max_h(px((height - 100.).min(650.)));
        if let overlays::ControllerPopup::Preset(d) = dialog {
            let title = match d.operation {
                PresetOperation::New => "New preset",
                PresetOperation::Rename => "Rename preset",
                PresetOperation::Duplicate => "Save a separate copy",
                PresetOperation::Edit => "Edit preset",
                PresetOperation::Endpoints => "Device ports",
                PresetOperation::Delete => "Delete this user preset?",
                PresetOperation::Feature => "Read feature report",
            };
            panel = panel.child(title);
            if matches!(d.operation, PresetOperation::Endpoints) {
                let ports = d.ports.clone();
                panel = panel.child(self.button(
                    "copy-endpoint-ids",
                    "Copy available port IDs",
                    true,
                    move |_, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(ports.clone()))
                    },
                    cx,
                ));
            }
            if !matches!(d.operation, PresetOperation::Delete) {
                let help = match d.operation {
                    PresetOperation::Edit | PresetOperation::Endpoints => {
                        "⌘Enter to save. Esc to cancel."
                    }
                    PresetOperation::Feature => "Report ID and length in bytes. Enter to read.",
                    _ => "Enter to save. Esc to cancel.",
                };
                panel = panel.child(self.preset_input(d, height, window, cx)).child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child(help),
                );
            }
            panel = panel.child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(self.button(
                        "cancel-preset-edit",
                        "Cancel",
                        true,
                        |s, _, _| s.preset_dialog = None,
                        cx,
                    ))
                    .child(self.button(
                        "save-preset-edit",
                        if matches!(d.operation, PresetOperation::Delete) {
                            "Delete"
                        } else {
                            "Save"
                        },
                        true,
                        |s, _, _| s.finish_preset_dialog(),
                        cx,
                    )),
            );
        }
        if self.error {
            panel = panel.child(div().text_color(theme::DANGER).child(self.status.clone()));
        }
        Self::controller_dialog_overlay(panel.into_any_element())
    }

    fn controller_dialog_overlay(panel: gpui::AnyElement) -> gpui::AnyElement {
        div()
            .id("controller-dialog-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .pt(px(TITLEBAR_HEIGHT))
            .pb_6()
            .px_6()
            .bg(gpui::rgba(0x00000070))
            .child(panel)
            .into_any_element()
    }
}
