use super::{MasterParam, graphs};
use crate::{
    controls::{KnobSpec, knob},
    state::{KnobCtl, UiState},
    theme,
    views::TextTip,
};
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px, svg};

pub(super) fn text(label: impl Into<SharedString>, size: f32, color: gpui::Rgba) -> gpui::Div {
    div()
        .text_size(px(size))
        .text_color(color)
        .child(label.into())
}
pub(super) fn row() -> gpui::Div {
    div().flex().items_center().gap_2()
}
pub(super) fn section() -> gpui::Div {
    div().flex().flex_col().gap_2().flex_1().min_w_0()
}
pub(super) fn axis(left: &str, right: &str) -> gpui::Div {
    div()
        .flex()
        .justify_between()
        .child(text(left.to_owned(), 9., theme::MUTED))
        .child(text(right.to_owned(), 9., theme::MUTED))
}

pub(super) fn stereo_meter(label: &'static str, levels: [f32; 2], color: gpui::Rgba) -> gpui::Div {
    row()
        .justify_between()
        .child(text(label, 9., theme::MUTED).w(px(46.)))
        .child(div().flex().flex_col().gap(px(3.)).flex_1().children(
            levels.into_iter().zip(["L", "R"]).map(|(level, label)| {
                row().gap_1().child(text(label, 8., theme::MUTED)).child(
                    div().h(px(4.)).flex_1().bg(theme::PANEL_INSET).child(
                        div()
                            .h_full()
                            .w(gpui::relative(
                                ((graphs::db(level) + 60.) / 60.).clamp(0., 1.),
                            ))
                            .bg(color),
                    ),
                )
            }),
        ))
        .child(
            text(graphs::db_text(levels[0].max(levels[1])), 10., color)
                .w(px(72.))
                .text_right(),
        )
}

impl UiState {
    pub(super) fn master_knob(&self, p: MasterParam, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .w(px(86.))
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::MasterProcessor(p),
                    value: p.value(self.snapshot.master_chain),
                    min: 0.,
                    max: 1.,
                    diameter: 27.,
                    color: theme::ACCENT,
                    label: p.label(),
                    bipolar: matches!(p, MasterParam::EqGain(_)),
                },
                cx,
            ))
            .child(text(
                p.display(self.snapshot.master_chain),
                10.,
                theme::TEXT,
            ))
            .into_any_element()
    }
    pub(super) fn master_toggle(
        &self,
        label: &'static str,
        which: usize,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        row()
            .justify_between()
            .child(text(label, 13., theme::TEXT))
            .child(
                div()
                    .id(SharedString::from(format!("master-toggle-{which}")))
                    .debug_selector(move || format!("master-toggle-{which}").into())
                    .cursor_pointer()
                    .px_3()
                    .py_1()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(if enabled { theme::ACCENT } else { theme::LINE })
                    .bg(if enabled {
                        theme::PANEL_RAISED
                    } else {
                        theme::PANEL_INSET
                    })
                    .text_size(px(10.))
                    .text_color(if enabled { theme::ACCENT } else { theme::MUTED })
                    .child(if enabled { "ON" } else { "OFF" })
                    .on_click(cx.listener(move |s, _, _, cx| {
                        let mut p = s.snapshot.master_chain;
                        match which {
                            0 => p.eq_enabled = !p.eq_enabled,
                            1 => p.compressor.enabled = !p.compressor.enabled,
                            _ => p.dynamics.enabled = !p.dynamics.enabled,
                        }
                        s.set_master_chain(p);
                        s.save_master_chain();
                        cx.notify();
                    })),
            )
            .into_any_element()
    }

    pub fn render_master_mixer(
        &self,
        size: gpui::Size<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let width = (f32::from(size.width) - 40.).min(1080.);
        let height = (f32::from(size.height) - 40.).min(840.);
        let eq = self.master_eq_panel(cx);
        let compressor = self.master_compressor_panel(cx);
        let dynamics = self.master_dynamics_panel(cx);
        let limiter = self.master_limiter_panel(cx);
        let mut master_levels = row().flex_none().gap_4();
        for (ctl, value, min, max, label, display) in [
            (
                KnobCtl::MasterGain,
                self.snapshot.master_gain_db,
                -12.,
                12.,
                "INPUT",
                format!("{:+.1} dB", self.snapshot.master_gain_db),
            ),
            (
                KnobCtl::Master,
                self.snapshot.master,
                0.,
                1.,
                "OUTPUT",
                format!("{:.0}%", self.snapshot.master * 100.),
            ),
        ] {
            master_levels = master_levels.child(
                row()
                    .child(knob(
                        KnobSpec {
                            ctl,
                            value,
                            min,
                            max,
                            diameter: 22.,
                            color: theme::LED_GREEN,
                            label: "",
                            bipolar: min < 0.,
                        },
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(text(label, 8., theme::MUTED))
                            .child(text(display, 11., theme::TEXT)),
                    ),
            );
        }
        let panel = div()
            .id("master-mixer-panel")
            .debug_selector(|| "master-mixer-panel".into())
            .w(px(width))
            .max_h(px(height))
            .flex()
            .flex_col()
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .rounded(px(5.))
            .overflow_hidden()
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                row()
                    .flex_none()
                    .justify_between()
                    .px_4()
                    .py_3()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(text("Master mixer", 18., theme::TEXT).flex_1())
                    .child(
                        row().flex_none().gap_5().child(master_levels).child(
                            div()
                                .id("master-mixer-close")
                                .debug_selector(|| "master-mixer-close".into())
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(28.))
                                .rounded(px(4.))
                                .hover(|s| s.bg(theme::PANEL_RAISED))
                                .child(
                                    svg()
                                        .path("icons/close.svg")
                                        .size(px(16.))
                                        .text_color(theme::MUTED),
                                )
                                .tooltip(|_, cx| cx.new(|_| TextTip("Close".into())).into())
                                .on_click(cx.listener(|s, _, _, cx| {
                                    s.close_master_mixer();
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .child(
                div()
                    .id("master-mixer-scroll")
                    .overflow_y_scroll()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .p_4()
                    .when(!self.error.is_empty(), |el| {
                        el.child(text(self.error.clone(), 11., theme::DANGER))
                    })
                    .child(eq)
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .flex_none()
                            .pt_4()
                            .border_t_1()
                            .border_color(theme::LINE)
                            .child(compressor)
                            .child(dynamics)
                            .child(limiter),
                    ),
            );
        div()
            .id("master-mixer-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x000000b8))
            .on_click(cx.listener(|s, _, _, cx| {
                s.close_master_mixer();
                cx.notify();
            }))
            .child(panel)
            .into_any_element()
    }
}
