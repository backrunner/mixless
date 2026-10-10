//! Shared settings geometry: readable rows, grouped surfaces and segmented choices.
use super::*;

pub(super) const CONTROL_HEIGHT: f32 = 32.;
pub(super) const CONTROL_RADIUS: f32 = 5.;

/// Small vector chevrons share a baseline regardless of the selected font.
pub(super) fn select_indicator(popup: bool, enabled: bool) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            let mut path = gpui::PathBuilder::stroke(px(1.25));
            let mut chevron = |y: f32, direction: f32| {
                path.move_to(center + gpui::point(px(-3.), px(y)));
                path.line_to(center + gpui::point(px(0.), px(y + direction * 3.)));
                path.line_to(center + gpui::point(px(3.), px(y)));
            };
            if popup {
                chevron(-2., -1.);
                chevron(2., 1.);
            } else {
                chevron(-1.5, 1.);
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, if enabled { theme::TEXT } else { theme::MUTED });
            }
        },
    )
    .w(px(12.))
    .h(px(16.))
    .flex_none()
}

fn disclosure_indicator(expanded: bool, enabled: bool) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| (),
        move |bounds, _, window, _| {
            let center = bounds.center();
            let mut path = gpui::PathBuilder::stroke(px(1.25));
            let points = if expanded {
                [(-3., -1.5), (0., 1.5), (3., -1.5)]
            } else {
                [(-1.5, -3.), (1.5, 0.), (-1.5, 3.)]
            };
            path.move_to(center + gpui::point(px(points[0].0), px(points[0].1)));
            for (x, y) in &points[1..] {
                path.line_to(center + gpui::point(px(*x), px(*y)));
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, if enabled { theme::TEXT } else { theme::MUTED });
            }
        },
    )
    .w(px(12.))
    .h(px(16.))
    .flex_none()
}

pub(super) fn setting_stack() -> gpui::Div {
    div().flex().flex_col().min_w_0().gap(px(20.))
}

pub(super) fn setting_group(title: &str, rows: Vec<gpui::Div>) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .min_w_0()
        .gap(px(8.))
        .child(
            div()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(theme::TEXT)
                .child(title.to_owned()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .border_t_1()
                .border_color(theme::LINE)
                .overflow_hidden()
                .children(rows.into_iter().enumerate().map(|(index, row)| {
                    row.border_b_0().when(index > 0, |el| {
                        el.border_t_1().border_color(theme::LINE_SOFT)
                    })
                })),
        )
}

pub(super) fn setting_detail(label: &str, detail: &str, control: gpui::AnyElement) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(px(52.))
        .px_3()
        .py_2()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.))
                        .text_color(theme::TEXT)
                        .child(label.to_owned()),
                )
                .when(!detail.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(12.))
                            .line_height(px(18.))
                            .text_color(theme::MUTED)
                            .child(detail.to_owned()),
                    )
                }),
        )
        .child(control)
}

impl Preferences {
    pub(super) fn disclosure_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        expanded: bool,
        enabled: bool,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.button(id, label, enabled, action, cx)
            .justify_between()
            .pr_2()
            .gap_2()
            .when(expanded, |el| {
                el.bg(theme::LINE)
                    .border_color(theme::with_alpha(theme::TEXT, 0.24))
            })
            .child(disclosure_indicator(expanded, enabled))
    }

    pub(super) fn segmented<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: &[(T, &'static str)],
        selected: T,
        change: fn(&mut Self, T),
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .debug_selector(move || format!("{id}-switcher"))
            .flex()
            .flex_none()
            .items_center()
            .p(px(3.))
            .gap(px(2.))
            .rounded(px(7.))
            .border_1()
            .border_color(theme::with_alpha(theme::TEXT, 0.08))
            .bg(theme::PANEL)
            .children(options.iter().enumerate().map(|(index, &(value, label))| {
                let active = value == selected;
                div()
                    .id(SharedString::from(format!("{id}-{index}")))
                    .debug_selector(move || format!("{id}-{index}"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(26.))
                    .px(px(12.))
                    .flex_none()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if active {
                        theme::with_alpha(theme::TEXT, 0.12)
                    } else {
                        gpui::transparent_black().into()
                    })
                    .bg(if active { theme::LINE } else { theme::PANEL })
                    .text_size(px(12.))
                    .text_color(if active { theme::TEXT } else { theme::MUTED })
                    .when(active, |el| {
                        el.font_weight(gpui::FontWeight::MEDIUM).shadow_sm()
                    })
                    .cursor_pointer()
                    .hover(move |s| {
                        s.text_color(theme::TEXT).bg(if active {
                            theme::LINE
                        } else {
                            theme::PANEL_RAISED
                        })
                    })
                    .child(label)
                    .on_click(cx.listener(move |s, _, _, cx| {
                        change(s, value);
                        cx.notify();
                    }))
            }))
            .into_any_element()
    }
}

impl Preferences {
    pub(super) fn reset_dialog(&self, cx: &Context<Self>) -> gpui::AnyElement {
        div().id("reset-dialog").absolute().inset_0().occlude().flex().items_center().justify_center().p_6()
            .bg(gpui::rgba(0x00000070))
            .child(div().flex().flex_col().gap_4().w(px(420.)).p_5().rounded(px(theme::DIALOG_RADIUS))
                .bg(theme::PANEL_RAISED).border_1().border_color(theme::LINE).shadow_lg()
                .child(div().text_size(px(16.)).font_weight(gpui::FontWeight::MEDIUM).child("Reset preferences?"))
                .child(div().text_size(px(12.)).line_height(px(18.)).text_color(theme::MUTED)
                    .child("Restore workspace, playback and audio settings. Your library and controller presets are kept. Audio output may briefly stop."))
                .child(div().flex().justify_end().gap_2()
                    .child(self.button("cancel-reset", "Cancel", true, |s, _, _| s.confirm_reset = false, cx))
                    .child(self.button("confirm-reset", "Reset preferences", self.job.is_none(), |s, _, _| {
                        s.confirm_reset = false; s.reset_defaults();
                    }, cx).text_color(theme::DANGER))))
            .into_any_element()
    }
}
