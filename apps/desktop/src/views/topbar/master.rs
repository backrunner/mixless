//! Independent limiter input gain and final master output level.
use crate::{
    controls::{KnobSpec, knob},
    state::{KnobCtl, UiState},
    theme,
};
use gpui::{IntoElement, prelude::*, px};

impl UiState {
    pub(super) fn render_master_controls(&self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let mut panel = gpui::div()
            .id("master-control")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .h(px(32.))
            .px_2()
            .rounded(px(6.))
            .border_1()
            .border_color(theme::with_alpha(theme::LED_GREEN, 0.24))
            .bg(theme::PANEL_INSET)
            // Prevent a drag near a knob from moving the native titlebar.
            .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .child(
                gpui::div()
                    .text_size(px(7.))
                    .text_color(theme::MUTED)
                    .child("MASTER"),
            );
        for (ctl, value, min, max, label, readout_width, readout) in [
            (
                KnobCtl::MasterGain,
                self.snapshot.master_gain_db,
                -12.,
                12.,
                "GAIN",
                43.,
                format!("{:+.1} dB", self.snapshot.master_gain_db),
            ),
            (
                KnobCtl::Master,
                self.snapshot.master,
                0.,
                1.,
                "LEVEL",
                30.,
                format!("{:.0}%", self.snapshot.master.clamp(0., 1.) * 100.),
            ),
        ] {
            panel = panel.child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_1()
                    .child(
                        gpui::div()
                            .w(px(readout_width))
                            .flex_col()
                            .flex()
                            .gap(px(1.))
                            .child(
                                gpui::div()
                                    .text_size(px(7.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(theme::MUTED)
                                    .child(label),
                            )
                            .child(
                                gpui::div()
                                    .text_size(px(10.))
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::LED_GREEN)
                                    .child(readout),
                            ),
                    )
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
                    )),
            );
        }
        panel.into_any_element()
    }
}
