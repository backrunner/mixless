//! A scrollable import review with pinned confirmation actions.
use super::*;
use crate::views::{TextTip, ellipsized_text};
use mixless_controllers::{ImportReport, community::ReportAccess};

pub(super) fn value(id: String, text: String) -> gpui::Stateful<gpui::Div> {
    div()
        .id(SharedString::from(id))
        .min_w_0()
        .h(px(18.))
        .child(ellipsized_text(text.clone()).w_full().h_full())
        .tooltip(move |_, cx| cx.new(|_| TextTip(text.clone())).into())
}

fn field(label: &'static str, text: String) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_w_0()
        .w_0()
        .gap_1()
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme::MUTED)
                .child(label),
        )
        .child(value(format!("import-field-{label}"), text).text_size(px(13.)))
}

fn section(title: &'static str, content: gpui::Div) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .min_w_0()
        .gap_2()
        .child(div().font_weight(gpui::FontWeight::MEDIUM).child(title))
        .child(content.border_t_1().border_color(theme::LINE))
}

pub(super) fn import_icon() -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        |bounds, _, window, _| {
            let mut path = gpui::PathBuilder::stroke(px(1.4));
            for points in [
                &[
                    (8., 3.),
                    (19., 3.),
                    (25., 9.),
                    (25., 29.),
                    (8., 29.),
                    (8., 3.),
                ][..],
                &[(19., 3.), (19., 9.), (25., 9.)],
                &[(16., 13.), (16., 23.)],
                &[(12., 19.), (16., 23.), (20., 19.)],
            ] {
                path.move_to(bounds.origin + gpui::point(px(points[0].0), px(points[0].1)));
                for &(x, y) in &points[1..] {
                    path.line_to(bounds.origin + gpui::point(px(x), px(y)));
                }
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, theme::MUTED);
            }
        },
    )
    .w(px(32.))
    .h(px(32.))
    .flex_none()
}

impl Preferences {
    pub(super) fn preset_import_panel(
        &self,
        report: &ImportReport,
        height: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let preset = &report.preset;
        let adapter = preset.adapter.as_ref();
        let mapping_type = if adapter.is_some() {
            "HID adapter"
        } else {
            match (
                preset
                    .endpoints
                    .iter()
                    .any(|e| matches!(e.matcher, DeviceMatcher::Midi { .. })),
                preset
                    .endpoints
                    .iter()
                    .any(|e| matches!(e.matcher, DeviceMatcher::Hid { .. })),
            ) {
                (true, true) => "MIDI / HID mapping",
                (true, false) => "MIDI mapping",
                (false, true) => "HID mapping",
                _ => "Mapping",
            }
        };
        let devices = div()
            .flex()
            .flex_col()
            .when(preset.endpoints.is_empty(), |el| {
                el.child(
                    div()
                        .py_2()
                        .text_color(theme::MUTED)
                        .child("No device requirements"),
                )
            })
            .children(preset.endpoints.iter().enumerate().map(|(i, e)| {
                let (protocol, name, detail) = match &e.matcher {
                    DeviceMatcher::Midi { names } => (
                        "MIDI",
                        if names.is_empty() {
                            "Any MIDI device".into()
                        } else {
                            names.join(", ")
                        },
                        String::new(),
                    ),
                    DeviceMatcher::Hid {
                        vendor_id,
                        product_id,
                        interface,
                        usage_page,
                        usage,
                    } => {
                        let mut details = Vec::new();
                        if let Some(v) = interface {
                            details.push(format!("Interface {v}"));
                        }
                        if let Some(v) = usage_page {
                            details.push(format!("Usage page {v}"));
                        }
                        if let Some(v) = usage {
                            details.push(format!("Usage {v}"));
                        }
                        (
                            "HID",
                            format!("{vendor_id:04X}:{product_id:04X}"),
                            details.join(", "),
                        )
                    }
                };
                div()
                    .debug_selector(move || format!("import-device-{i}"))
                    .flex()
                    .items_center()
                    .gap_3()
                    .min_w_0()
                    .py_2()
                    .when(i > 0, |el| el.border_t_1().border_color(theme::LINE_SOFT))
                    .child(
                        value(format!("import-port-{i}"), e.id.clone())
                            .w(px(100.))
                            .flex_none()
                            .text_color(theme::MUTED),
                    )
                    .child(
                        div()
                            .w(px(40.))
                            .flex_none()
                            .text_size(px(11.))
                            .text_color(theme::MUTED)
                            .child(protocol),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .w_0()
                            .min_w_0()
                            .gap_1()
                            .child(value(format!("import-device-name-{i}"), name))
                            .when(!detail.is_empty(), |el| {
                                el.child(
                                    value(format!("import-device-detail-{i}"), detail)
                                        .text_size(px(11.))
                                        .text_color(theme::MUTED),
                                )
                            }),
                    )
            }));
        let body = div()
            .id("preset-import-scroll")
            .debug_selector(|| "preset-import-scroll".into())
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scroll()
            .px_5()
            .py_4()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_5()
                    .min_w_0()
                    .when_some(preset.community.as_ref(), |el, c| {
                        el.child(
                            div()
                                .debug_selector(|| "import-community".into())
                                .flex()
                                .items_end()
                                .gap_4()
                                .child(field("Author", c.author.clone()))
                                .child(field("Version", c.package_version.clone()))
                                .child(field("License", c.license.clone()))
                                .child(self.button(
                                    "import-source",
                                    "View source",
                                    true,
                                    {
                                        let url = c.source_url.clone();
                                        move |_, _, cx| cx.open_url(&url)
                                    },
                                    cx,
                                )),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap_4()
                            .child(field("Type", mapping_type.into()))
                            .child(field(
                                "Controls",
                                adapter
                                    .map_or_else(|| preset.bindings.len(), |a| a.controls.len())
                                    .to_string(),
                            ))
                            .child(field(
                                "Feedback",
                                adapter.map_or_else(
                                    || preset.feedback.len().to_string(),
                                    |_| "Adapter".into(),
                                ),
                            )),
                    )
                    .child(section("Devices", devices))
                    .when_some(adapter, |el, a| {
                        let cell =
                            |text: String, width: f32| div().w(px(width)).flex_none().child(text);
                        let header = div()
                            .flex()
                            .items_center()
                            .gap_3()
                            .h(px(30.))
                            .text_size(px(11.))
                            .text_color(theme::MUTED)
                            .child(cell("Port".into(), 100.))
                            .child(div().flex_1().min_w_0().w_0().child("Access"))
                            .child(cell("Report ID".into(), 64.))
                            .child(cell("Bytes".into(), 48.));
                        let rows = div()
                            .debug_selector(|| "import-adapter-access".into())
                            .flex()
                            .flex_col()
                            .child(header)
                            .children(a.reports.iter().enumerate().map(|(i, r)| {
                                div()
                                    .debug_selector(move || format!("import-report-{i}"))
                                    .flex()
                                    .items_center()
                                    .gap_3()
                                    .h(px(32.))
                                    .flex_none()
                                    .min_w_0()
                                    .border_t_1()
                                    .border_color(theme::LINE_SOFT)
                                    .child(
                                        value(
                                            format!("import-report-port-{i}"),
                                            r.endpoint.clone(),
                                        )
                                        .w(px(100.))
                                        .flex_none()
                                        .text_color(theme::MUTED),
                                    )
                                    .child(div().flex_1().min_w_0().w_0().child(match r.access {
                                        ReportAccess::Output => "Output",
                                        ReportAccess::FeatureWrite => "Write feature",
                                        ReportAccess::FeatureRead => "Read feature",
                                    }))
                                    .child(cell(r.report_id.to_string(), 64.))
                                    .child(cell(r.length.to_string(), 48.))
                            }))
                            .when(a.reports.is_empty(), |el| {
                                el.child(
                                    div()
                                        .py_2()
                                        .text_color(theme::MUTED)
                                        .child("No output or feature access"),
                                )
                            });
                        el.child(section("Report access", rows)).child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::MUTED)
                                .child("The adapter cannot access files or the network."),
                        )
                    })
                    .when(!report.conflicts.is_empty(), |el| {
                        el.child(section(
                            "Existing presets",
                            div().flex().flex_col().gap_1().pt_2().children(
                                report
                                    .conflicts
                                    .iter()
                                    .map(|c| div().text_size(px(12.)).child(c.clone())),
                            ),
                        ))
                    }),
            );
        // Keep failure details and confirmation reachable even for a 128-report manifest.
        let footer = div()
            .debug_selector(|| "preset-import-footer".into())
            .flex()
            .flex_col()
            .flex_none()
            .border_t_1()
            .border_color(theme::LINE)
            .px_5()
            .py_4()
            .gap_3()
            .when(self.error, |el| {
                el.child(
                    div()
                        .id("preset-import-error")
                        .max_h(px(48.))
                        .overflow_y_scroll()
                        .text_size(px(11.))
                        .text_color(theme::DANGER)
                        .child(self.status.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .w_0()
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(theme::MUTED)
                            .child(if self.import_batch.is_some() {
                                "Return to the file list to import the batch."
                            } else {
                                "Saves a separate copy. Apply it to a device after import."
                            }),
                    )
                    .child(self.button(
                        "cancel-preset-import",
                        if self.import_batch.is_some() {
                            "Back"
                        } else {
                            "Cancel"
                        },
                        self.job.is_none(),
                        |s, _, _| s.import_preview = None,
                        cx,
                    ))
                    .when(self.import_batch.is_none(), |el| {
                        el.child(self.primary_button(
                            "accept-preset-import",
                            "Import preset",
                            self.job.is_none(),
                            |s, _, _| s.accept_preset_import(),
                            cx,
                        ))
                    }),
            );
        // Explicit width avoids percentage sizing against the scroll body's intrinsic width.
        div()
            .id("controller-dialog-panel")
            .debug_selector(|| "controller-dialog-panel".into())
            .flex()
            .flex_col()
            .w(px((f32::from(window.viewport_size().width) - 48.).min(720.)))
            .max_h(px((height - 100.).min(600.)))
            .min_w_0()
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .rounded(px(6.))
            .shadow_lg()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .flex_none()
                    .p_5()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(import_icon())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .flex_1()
                            .w_0()
                            .min_w_0()
                            .child(div().text_size(px(11.)).text_color(theme::MUTED).child(
                                if self.import_batch.is_some() {
                                    "Preset details"
                                } else {
                                    "Import preset"
                                },
                            ))
                            .child(
                                value("import-preset-name".into(), preset.name.clone())
                                    .h(px(24.))
                                    .text_size(px(18.))
                                    .font_weight(gpui::FontWeight::MEDIUM),
                            ),
                    ),
            )
            .child(body)
            .child(footer)
            .into_any_element()
    }
}
