//! Batch file results and import actions, separate from per-preset details.
use super::import::{import_icon, value};
use super::*;
use mixless_controllers::ImportBatch;

impl Preferences {
    pub(super) fn accept_preset_batch(&mut self) {
        if self.job.is_some() {
            return;
        }
        let Some(batch) = &self.import_batch else {
            return;
        };
        let reports: Vec<_> = batch
            .entries
            .iter()
            .filter_map(|e| e.result.as_ref().ok())
            .cloned()
            .collect();
        if reports.is_empty() {
            return;
        }
        let skipped = batch.skipped_count();
        match self.hub() {
            Ok(hub) => {
                self.result(Ok(()), "Importing presets…");
                self.start_job(move || {
                    JobResult::PresetsImported(
                        hub.accept_imports(reports).map_err(|e| e.to_string()),
                        skipped,
                    )
                });
            }
            Err(e) => self.result(Err(e), ""),
        }
    }

    pub(super) fn preset_batch_panel(
        &self,
        batch: &ImportBatch,
        height: f32,
        window: &Window,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let ready = batch.ready_count();
        let skipped = batch.skipped_count();
        let idle = self.job.is_none();
        let rows = batch.entries.iter().enumerate().map(|(index, entry)| {
            let (name, detail, color) = match &entry.result {
                Ok(report) => {
                    let p = &report.preset;
                    let detail = if let Some(a) = &p.adapter {
                        format!(
                            "HID adapter, {} controls, {} report permissions",
                            a.controls.len(),
                            a.reports.len()
                        )
                    } else {
                        format!("{} controls", p.bindings.len())
                    };
                    (preset_label(&p.name).to_owned(), detail, theme::MUTED)
                }
                Err(error) => (entry.file_name.clone(), error.clone(), theme::DANGER),
            };
            let valid = entry.result.is_ok();
            div()
                .debug_selector(move || format!("import-file-{index}"))
                .flex()
                .items_center()
                .gap_3()
                .min_w_0()
                .py_3()
                .when(index > 0, |el| {
                    el.border_t_1().border_color(theme::LINE_SOFT)
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .w_0()
                        .min_w_0()
                        .gap_1()
                        .child(
                            value(format!("import-file-name-{index}"), name)
                                .font_weight(gpui::FontWeight::MEDIUM),
                        )
                        .when(entry.result.is_ok(), |el| {
                            el.child(
                                value(
                                    format!("import-file-source-{index}"),
                                    entry.file_name.clone(),
                                )
                                .text_size(px(11.))
                                .text_color(theme::MUTED),
                            )
                        })
                        .child(
                            value(format!("import-file-result-{index}"), detail)
                                .text_size(px(11.))
                                .text_color(color),
                        )
                        .when(
                            entry.result.as_ref().is_ok_and(|r| !r.conflicts.is_empty()),
                            |el| {
                                el.child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(theme::ACCENT)
                                        .child("A separate copy will be created"),
                                )
                            },
                        ),
                )
                .child(
                    div()
                        .w(px(56.))
                        .flex_none()
                        .text_size(px(11.))
                        .text_color(color)
                        .child(if valid { "Ready" } else { "Skipped" }),
                )
                .when(valid, |el| {
                    el.child(self.button(
                        format!("review-import-{index}"),
                        "Details",
                        idle,
                        move |s, _, _| {
                            s.import_preview = s
                                .import_batch
                                .as_ref()
                                .and_then(|b| b.entries.get(index))
                                .and_then(|e| e.result.as_ref().ok())
                                .cloned();
                        },
                        cx,
                    ))
                })
        });
        let body = div()
            .id("preset-batch-scroll")
            .debug_selector(|| "preset-batch-scroll".into())
            .flex_1()
            .min_h_0()
            .min_w_0()
            .overflow_y_scroll()
            .px_5()
            .children(rows);
        let footer = div()
            .debug_selector(|| "preset-import-footer".into())
            .flex()
            .flex_col()
            .flex_none()
            .gap_3()
            .px_5()
            .py_4()
            .border_t_1()
            .border_color(theme::LINE)
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
                            .w_0()
                            .min_w_0()
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(theme::MUTED)
                            .child(if ready > 0 {
                                "Saves separate copies. Apply a preset to a device after import."
                            } else {
                                "No valid presets to import."
                            }),
                    )
                    .child(self.button(
                        "cancel-preset-import",
                        "Cancel",
                        idle,
                        |s, _, _| {
                            s.import_preview = None;
                            s.import_batch = None;
                        },
                        cx,
                    ))
                    .child(self.primary_button(
                        "accept-preset-import",
                        if idle {
                            format!(
                                "Import {ready} {}",
                                if ready == 1 { "preset" } else { "presets" }
                            )
                        } else {
                            "Importing…".into()
                        },
                        idle && ready > 0,
                        |s, _, _| s.accept_preset_batch(),
                        cx,
                    )),
            );
        div()
            .id("controller-dialog-panel")
            .debug_selector(|| "controller-dialog-panel".into())
            .flex()
            .flex_col()
            .min_w_0()
            .w(px((f32::from(window.viewport_size().width) - 48.).min(720.)))
            .max_h(px((height - 100.).min(600.)))
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
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .child(
                                div()
                                    .text_size(px(18.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .child("Import presets"),
                            )
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::MUTED)
                                    .child(format!("{ready} ready, {skipped} skipped")),
                            ),
                    ),
            )
            .child(body)
            .child(footer)
            .into_any_element()
    }
}
