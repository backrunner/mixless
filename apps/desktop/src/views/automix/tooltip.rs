//! Bounded, hoverable transition detail. Each stage owns its wrapped height.
use super::*;

pub(super) struct AutomixTip {
    pub transition: Transition,
    pub gap: f32,
    pub _subscription: Option<gpui::Subscription>,
}

impl AutomixTip {
    pub fn new(
        transition: Transition,
        gap: f32,
        state: gpui::WeakEntity<UiState>,
        cx: &mut gpui::Context<Self>,
    ) -> Self {
        let subscription = state.upgrade().map(|state| {
            cx.observe(&state, |tip, state, cx| {
                if let Some(transition) = state.read(cx).transition()
                    && transition != tip.transition
                {
                    tip.transition = transition;
                    cx.notify();
                }
            })
        });
        Self {
            transition,
            gap,
            _subscription: subscription,
        }
    }
}

fn field(label: String, value: String, color: gpui::Rgba) -> gpui::Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(div().text_size(px(9.)).text_color(color).child(label))
        .child(div().text_color(theme::TEXT).child(value))
}

impl gpui::Render for AutomixTip {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        _: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let t = &self.transition;
        let live = if t.paused {
            theme::WARN
        } else {
            theme::LED_GREEN
        };
        let max_height =
            (f32::from(window.viewport_size().height) - self.gap - 80.).clamp(120., 320.);
        div().w(px(284.)).pt(px(self.gap)).child(
            div()
                .id("automix-detail")
                .debug_selector(|| "automix-detail".into())
                .w_full()
                .max_h(px(max_height))
                .overflow_y_scroll()
                .p(px(12.))
                .rounded(px(6.))
                .bg(theme::PANEL_RAISED)
                .border_1()
                .border_color(theme::LINE)
                .shadow_md()
                .flex()
                .flex_col()
                .gap(px(10.))
                .text_size(px(11.))
                .line_height(px(15.))
                .text_color(theme::MUTED)
                .whitespace_normal()
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(5.))
                                .text_size(px(12.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(
                                    div()
                                        .text_color(theme::deck_color(t.out))
                                        .child(label(t.out)),
                                )
                                .child("→")
                                .child(
                                    div()
                                        .text_color(theme::deck_color(t.incoming))
                                        .child(label(t.incoming)),
                                )
                                .child(div().ml(px(4.)).text_color(theme::TEXT).child(t.name)),
                        )
                        .child(div().text_color(live).child(t.phase.clone())),
                )
                .child(
                    div()
                        .flex_none()
                        .flex()
                        .gap(px(12.))
                        .pt(px(8.))
                        .border_t_1()
                        .border_color(theme::LINE)
                        .child(field(
                            format!("{} OUT", label(t.out)),
                            t.windows[0].clone(),
                            theme::deck_color(t.out),
                        ))
                        .child(field(
                            format!("{} IN", label(t.incoming)),
                            t.windows[1].clone(),
                            theme::deck_color(t.incoming),
                        )),
                )
                .child(
                    div().flex_none().flex().gap(px(12.)).children(
                        ["BPM", "KEY", "LEVEL"].into_iter().zip(&t.parameters).map(
                            |(name, value)| {
                                field(
                                    format!("{} {name}", label(t.incoming)),
                                    value.clone(),
                                    theme::MUTED.into(),
                                )
                            },
                        ),
                    ),
                )
                .when(!t.stages.is_empty(), |tip| {
                    tip.child(
                        div()
                            .flex_none()
                            .w_full()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .pt(px(8.))
                            .border_t_1()
                            .border_color(theme::LINE)
                            .children(t.stages.iter().enumerate().map(|(i, stage)| {
                                div()
                                    .id(("automix-stage", i))
                                    .debug_selector(move || format!("automix-stage-{i}").into())
                                    .flex_none()
                                    .w_full()
                                    .flex()
                                    .items_start()
                                    .gap(px(6.))
                                    .text_size(px(10.))
                                    .line_height(px(14.))
                                    .text_color(if t.active_stage == Some(i) {
                                        theme::TEXT
                                    } else {
                                        theme::MUTED
                                    })
                                    .child(
                                        div()
                                            .flex_none()
                                            .w(px(10.))
                                            .text_color(if t.active_stage == Some(i) {
                                                live
                                            } else {
                                                theme::MUTED.into()
                                            })
                                            .child((i + 1).to_string()),
                                    )
                                    .child(div().flex_1().min_w_0().child(stage.clone()))
                            })),
                    )
                })
                .when_some(t.echo.as_ref(), |tip, echo| {
                    tip.child(
                        div()
                            .flex_none()
                            .text_size(px(10.))
                            .text_color(theme::ACCENT)
                            .child(echo.clone()),
                    )
                }),
        )
    }
}
