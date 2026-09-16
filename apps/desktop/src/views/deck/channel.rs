//! Per-deck channel strip beside the jog: GAIN mirrors the mixer's TRIM and
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
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::Gain(deck),
                    value: d.gain_db,
                    min: -12.0,
                    max: 12.0,
                    diameter,
                    color: theme::POINTER,
                    label: "GAIN",
                    bipolar: true,
                },
                cx,
            ))
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
