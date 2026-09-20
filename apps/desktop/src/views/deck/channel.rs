//! Per-deck channel strip beside the jog: GAIN drives the deck limiter and
//! BALANCE pans the channel's stereo output.

use gpui::{IntoElement, prelude::*, px};
use mixless_protocol::{DeckId, DeckSnapshot};

use crate::{
    controls::{KnobSpec, knob},
    state::{KnobCtl, UiState},
    theme,
};

impl UiState {
    pub(super) fn render_deck_channel(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        d: &DeckSnapshot,
        compact_height: bool,
    ) -> gpui::AnyElement {
        let diameter = if compact_height { 24.0 } else { 28.0 };
        gpui::div()
            .flex()
            .flex_none()
            .flex_col()
            .min_h_0()
            .w(px(50.))
            .items_center()
            .justify_between()
            .gap(px(10.))
            .py_1()
            .child(
                gpui::div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(knob(
                        KnobSpec {
                            ctl: KnobCtl::Gain(deck),
                            value: d.effective_limiter_gain_db(),
                            min: -12.0,
                            max: 12.0,
                            diameter,
                            color: theme::POINTER,
                            label: "GAIN",
                            bipolar: true,
                        },
                        cx,
                    ))
                    .child(
                        gpui::div()
                            .h(px(10.))
                            .text_size(px(7.))
                            .text_color(theme::LED_GREEN)
                            .child(if d.automix_gain_db >= 0.05 {
                                format!("AUTO +{:.1}", d.automix_gain_db)
                            } else {
                                String::new()
                            }),
                    ),
            )
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::Balance(deck),
                    value: d.balance,
                    min: -1.0,
                    max: 1.0,
                    diameter,
                    color: theme::POINTER,
                    label: "BALANCE",
                    bipolar: true,
                },
                cx,
            ))
            .into_any_element()
    }
}
