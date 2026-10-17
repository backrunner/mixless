//! Individual processor panels.
use super::view::{axis, row, section, stereo_meter, text};
use super::{MasterParam, graphs};
use crate::{state::UiState, theme};
use gpui::{Context, IntoElement, SharedString, div, prelude::*, px};

impl UiState {
    pub(super) fn master_eq_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use MasterParam::*;
        let p = self.snapshot.master_chain;
        let selected = self.master_mixer.band;
        let mut bands = row().gap_1();
        for i in 0..5 {
            bands = bands.child(
                div()
                    .id(("master-eq-band", i))
                    .cursor_pointer()
                    .px_3()
                    .py_1()
                    .bg(if selected == i {
                        theme::PANEL_RAISED
                    } else {
                        theme::PANEL
                    })
                    .border_b_1()
                    .border_color(if selected == i {
                        theme::ACCENT
                    } else {
                        theme::LINE
                    })
                    .child(text(
                        format!("{}  {}", i + 1, Frequency(i).display(p)),
                        10.,
                        if selected == i {
                            theme::ACCENT
                        } else {
                            theme::MUTED
                        },
                    ))
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.master_mixer.band = i;
                        cx.notify();
                    })),
            );
        }
        let eq = section()
            .flex_none()
            .child(self.master_toggle("PARAMETRIC EQ", 0, p.eq_enabled, cx))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        section().child(graphs::eq_graph(self, cx)).child(
                            div().flex().justify_between().children(
                                [
                                    "20 Hz", "63 Hz", "200 Hz", "630 Hz", "2 kHz", "6.3 kHz",
                                    "20 kHz",
                                ]
                                .map(|label| text(label, 9., theme::MUTED)),
                            ),
                        ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .justify_between()
                            .w(px(52.))
                            .pb_4()
                            .child(text("+12 dB", 9., theme::MUTED))
                            .child(text("0 dB", 9., theme::MUTED))
                            .child(text("−12 dB", 9., theme::MUTED)),
                    ),
            )
            .child(
                row().justify_between().child(bands).child(
                    row()
                        .child(self.master_knob(Frequency(selected), cx))
                        .child(self.master_knob(EqGain(selected), cx))
                        .child(self.master_knob(Q(selected), cx)),
                ),
            );

        eq.into_any_element()
    }
    pub(super) fn master_compressor_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use MasterParam::*;
        let p = self.snapshot.master_chain;
        let m = self.snapshot.master_meters;
        let mut compressor_knobs = row().flex_wrap().justify_center().gap_y_2();
        for param in [Threshold, Ratio, Knee, Attack, Release, Makeup] {
            compressor_knobs = compressor_knobs.child(self.master_knob(param, cx));
        }
        let compressor = section()
            .pr_4()
            .border_r_1()
            .border_color(theme::LINE)
            .child(self.master_toggle("COMPRESSOR", 1, p.compressor.enabled, cx))
            .child(
                div()
                    .bg(theme::PANEL_INSET)
                    .relative()
                    .child(graphs::transfer(p, m.compressor, false))
                    .child(
                        text("Output ↑", 9., theme::MUTED)
                            .absolute()
                            .top_1()
                            .left_2(),
                    ),
            )
            .child(axis("−80 dBFS   Input →", "0 dBFS"))
            .child(
                row()
                    .justify_between()
                    .child(text("GAIN REDUCTION", 9., theme::MUTED))
                    .child(text(
                        format!("{:.1} dB", m.compressor.reduction_db),
                        12.,
                        theme::ACCENT,
                    )),
            )
            .child(compressor_knobs);

        compressor.into_any_element()
    }
    pub(super) fn master_dynamics_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use MasterParam::*;
        let p = self.snapshot.master_chain;
        let m = self.snapshot.master_meters;
        let mut dynamics_knobs = row().flex_wrap().justify_center().gap_y_2();
        for param in [
            GateThreshold,
            GateRatio,
            Range,
            GateAttack,
            Hold,
            GateRelease,
        ]
        .into_iter()
        .filter(|param| !matches!(param, GateRatio) || !p.dynamics.gate)
        {
            dynamics_knobs = dynamics_knobs.child(self.master_knob(param, cx));
        }
        let modes =
            row()
                .gap_1()
                .children([(false, "Expander"), (true, "Gate")].into_iter().map(
                    |(gate, label)| {
                        div()
                            .id(SharedString::from(format!("master-dynamics-{label}")))
                            .cursor_pointer()
                            .px_2()
                            .py_1()
                            .text_size(px(10.))
                            .bg(if p.dynamics.gate == gate {
                                theme::PANEL_RAISED
                            } else {
                                theme::PANEL
                            })
                            .text_color(if p.dynamics.gate == gate {
                                theme::ACCENT
                            } else {
                                theme::MUTED
                            })
                            .child(label)
                            .on_click(cx.listener(move |s, _, _, cx| {
                                let mut p = s.snapshot.master_chain;
                                p.dynamics.gate = gate;
                                s.set_master_chain(p);
                                s.save_master_chain();
                                cx.notify();
                            }))
                    },
                ));
        let dynamics = section()
            .px_2()
            .pr_4()
            .border_r_1()
            .border_color(theme::LINE)
            .child(self.master_toggle("DYNAMICS", 2, p.dynamics.enabled, cx))
            .child(
                div()
                    .bg(theme::PANEL_INSET)
                    .relative()
                    .child(graphs::transfer(p, m.dynamics, true))
                    .child(
                        text("Output ↑", 9., theme::MUTED)
                            .absolute()
                            .top_1()
                            .left_2(),
                    ),
            )
            .child(axis("−80 dBFS   Input →", "0 dBFS"))
            .child(row().justify_between().child(modes).child(text(
                format!("{:.1} dB", m.dynamics.reduction_db),
                12.,
                theme::ACCENT,
            )))
            .child(dynamics_knobs);

        dynamics.into_any_element()
    }
    pub(super) fn master_limiter_panel(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use MasterParam::*;
        let p = self.snapshot.master_chain;
        let m = self.snapshot.master_meters;
        let limiter = section()
            .pl_2()
            .child(
                row()
                    .h(px(26.))
                    .justify_between()
                    .child(text("LIMITER", 13., theme::TEXT))
                    .child(text("ALWAYS ON", 9., theme::LED_GREEN)),
            )
            .child(div().bg(theme::PANEL_INSET).child(graphs::history(
                self.master_mixer.history.clone(),
                p.limiter.ceiling_db,
            )))
            .child(
                row()
                    .justify_between()
                    .child(text("IN", 9., theme::MUTED))
                    .child(text("OUT", 9., theme::LED_GREEN))
                    .child(text("REDUCTION", 9., theme::ACCENT))
                    .child(text("5 s", 9., theme::MUTED)),
            )
            .child(axis("Peak −60 to +6 dBFS", "Reduction 0–24 dB"))
            .child(
                row()
                    .justify_between()
                    .child(text("GAIN REDUCTION", 9., theme::MUTED))
                    .child(text(
                        format!("{:.1} dB", m.limiter.reduction_db),
                        12.,
                        theme::ACCENT,
                    )),
            )
            .child(
                row()
                    .justify_center()
                    .child(self.master_knob(Ceiling, cx))
                    .child(self.master_knob(LimiterRelease, cx)),
            )
            .child(stereo_meter("INPUT", m.limiter.input, theme::MUTED))
            .child(stereo_meter("OUTPUT", m.limiter.output, theme::LED_GREEN))
            .child(
                row()
                    .justify_between()
                    .child(text("OUTPUT RMS", 9., theme::MUTED))
                    .child(text(
                        graphs::db_text(m.rms[0].max(m.rms[1])),
                        11.,
                        theme::TEXT,
                    )),
            )
            .child(text(
                "Stereo linked   Sample peak   0 ms latency",
                9.,
                theme::MUTED,
            ));

        limiter.into_any_element()
    }
}
