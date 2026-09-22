//! Shared settings geometry: readable rows, grouped surfaces and segmented choices.
use super::*;

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
                .px_1()
                .text_size(px(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme::TEXT)
                .child(title.to_owned()),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .px_3()
                .rounded(px(8.))
                .bg(theme::PANEL)
                .border_1()
                .border_color(theme::LINE)
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
        .min_h(px(44.))
        .py_3()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(12.)).child(label.to_owned()))
                .when(!detail.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .text_color(theme::MUTED)
                            .child(detail.to_owned()),
                    )
                }),
        )
        .child(control)
}

impl Preferences {
    pub(super) fn segmented<T: Copy + PartialEq + 'static>(
        &self,
        id: &'static str,
        options: &[(T, &'static str)],
        selected: T,
        change: fn(&mut Self, T),
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .flex()
            .flex_none()
            .p(px(3.))
            .gap(px(2.))
            .rounded(px(6.))
            .bg(theme::PANEL_INSET)
            .border_1()
            .border_color(theme::LINE)
            .children(options.iter().enumerate().map(|(index, &(value, label))| {
                let active = value == selected;
                self.button(
                    format!("{id}-{index}"),
                    label,
                    true,
                    move |s, _, _| change(s, value),
                    cx,
                )
                .h(px(25.))
                .px(px(10.))
                .text_size(px(11.))
                .border_0()
                .rounded(px(3.))
                .bg(if active {
                    theme::PANEL_RAISED
                } else {
                    theme::PANEL_INSET
                })
                .text_color(if active { theme::TEXT } else { theme::MUTED })
                .when(active, |el| el.font_weight(gpui::FontWeight::MEDIUM))
            }))
            .into_any_element()
    }
}
