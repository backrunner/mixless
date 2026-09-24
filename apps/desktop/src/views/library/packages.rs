use crate::{
    state::{PackageAction, UiState},
    theme,
};
use gpui::{div, prelude::*, px};

impl UiState {
    pub(super) fn render_packages(&self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let busy = self.package.busy();
        let disabled = busy || self.picker_open;
        let button = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px_2()
                .h(px(26.))
                .flex()
                .items_center()
                .text_size(px(11.))
                .rounded(px(3.))
                .border_1()
                .border_color(theme::LINE)
                .bg(theme::PANEL_RAISED)
                .when(disabled, |el| el.opacity(0.45))
                .when(!disabled, |el| {
                    el.cursor_pointer().hover(|s| s.bg(theme::LINE))
                })
                .child(label)
        };
        let mut section = div()
            .flex()
            .flex_col()
            .gap_2()
            .pt_3()
            .border_t_1()
            .border_color(theme::LINE)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().text_size(px(11.)).child("Portable package"))
                    .child(button("package-import", "Import…").on_click(cx.listener(
                        move |s, _, _, cx| {
                            if !disabled {
                                s.choose_package(PackageAction::Import, cx);
                            }
                        },
                    ))),
            );
        section = section.child(
            div()
                .id("package-entire-library")
                .flex()
                .items_center()
                .gap_2()
                .text_size(px(11.))
                .cursor_pointer()
                .child(if self.package.all { "☑" } else { "☐" })
                .child("Entire library")
                .on_click(cx.listener(move |s, _, _, cx| {
                    if !busy {
                        s.package.all = !s.package.all;
                        cx.notify();
                    }
                })),
        );
        if !self.package.all {
            section = section.child(
                div()
                    .id("package-playlists")
                    .max_h(px(108.))
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(self.playlists.iter().map(|p| {
                        let id = p.id;
                        let checked = self.package.selected.contains(&id);
                        div()
                            .id(("package-playlist", id as usize))
                            .h(px(22.))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .gap_2()
                            .px_2()
                            .text_size(px(11.))
                            .cursor_pointer()
                            .hover(|s| s.bg(theme::PANEL_RAISED))
                            .child(if checked { "☑" } else { "☐" })
                            .child(
                                div()
                                    .truncate()
                                    .child(super::folder_name(p, &self.playlists)),
                            )
                            .on_click(cx.listener(move |s, _, _, cx| {
                                if !busy {
                                    if !s.package.selected.remove(&id) {
                                        s.package.selected.insert(id);
                                    }
                                    cx.notify();
                                }
                            }))
                    })),
            );
        }
        section = section.child(
            div()
                .flex()
                .gap_2()
                .child(button("package-export", "Export…").on_click(cx.listener(
                    move |s, _, _, cx| {
                        if !disabled {
                            s.choose_package(PackageAction::Export, cx);
                        }
                    },
                )))
                .child(
                    button("package-append", "Update existing…").on_click(cx.listener(
                        move |s, _, _, cx| {
                            if !disabled {
                                s.choose_package(PackageAction::Append, cx);
                            }
                        },
                    )),
                ),
        );
        if !self.package.status.is_empty() {
            section = section.child(
                div()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(self.package.status.clone()),
            );
        }
        if busy {
            section = section.child(
                div()
                    .id("package-cancel")
                    .text_size(px(11.))
                    .text_color(theme::TEXT)
                    .cursor_pointer()
                    .child("Cancel")
                    .on_click(cx.listener(|s, _, _, cx| {
                        s.cancel_package();
                        cx.notify();
                    })),
            );
        }
        section.into_any_element()
    }
}
