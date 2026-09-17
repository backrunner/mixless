//! Center mixer, Pioneer DJM style: TRIM + FILTER on top, HI/MID/LOW EQ
//! column with kill dots on the outer side, PFL, channel fader flanked by
//! dual LED meters, crossfader at the bottom.

use gpui::prelude::*;
use gpui::{IntoElement, SharedString, Styled, px};
use mixless_protocol::{DeckId, EqBand};

use crate::controls::{FaderSpec, KnobSpec, fader_v, knob, meter, xfader};
use crate::state::{FaderCtl, KnobCtl, UiState};
use crate::theme;
use crate::views::deck::deck_label;

impl UiState {
    pub fn render_mixer(
        &self,
        cx: &mut gpui::Context<Self>,
        width: f32,
        compact_height: bool,
    ) -> gpui::AnyElement {
        let channels = gpui::div()
            .flex()
            .flex_1()
            .min_h_0()
            .gap_1()
            .child(self.render_channel(cx, DeckId::A, compact_height))
            .child(self.render_channel(cx, DeckId::B, compact_height));

        let xf_section = gpui::div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .pt_2()
            .child(gpui::div().flex_1().h(px(1.)).bg(theme::LINE))
            .child(
                gpui::div()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::DECK_A)
                    .child("A"),
            )
            .child(xfader(self.snapshot.xfader, cx))
            .child(
                gpui::div()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::DECK_B)
                    .child("B"),
            )
            .child(gpui::div().flex_1().h(px(1.)).bg(theme::LINE));

        gpui::div()
            .id("mixer")
            .flex()
            .flex_none()
            .w(px(width))
            .min_w(px(width))
            .flex_col()
            .gap_1()
            .p_2()
            .rounded(px(8.))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .child(channels)
            .child(xf_section)
            .into_any_element()
    }

    fn render_channel(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        compact_height: bool,
    ) -> gpui::AnyElement {
        let d = self.deck(deck);
        let dc = theme::deck_color(deck);
        let flip = deck == DeckId::B;
        let pfl_available = self.snapshot.pfl_available;
        let top_diameter = 22.0;
        let eq_diameter = if compact_height { 26.0 } else { 30.0 };
        let eq_row_width = 56.0;

        /* top row: TRIM + FILTER side by side */
        let top_knobs = gpui::div()
            .flex()
            .flex_none()
            .items_start()
            .justify_center()
            .gap(px(6.0))
            .pt(px(if compact_height { 2.0 } else { 4.0 }))
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::Trim(deck),
                    value: d.gain_db,
                    min: -12.0,
                    max: 12.0,
                    diameter: top_diameter,
                    color: theme::POINTER,
                    label: "TRIM",
                    bipolar: true,
                },
                cx,
            ))
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::Filter(deck),
                    value: d.filter_amount,
                    min: -1.0,
                    max: 1.0,
                    diameter: top_diameter,
                    color: theme::WARN,
                    label: "FILTER",
                    bipolar: true,
                },
                cx,
            ))
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::Resonance(deck),
                    value: d.filter_resonance,
                    min: 0.0,
                    max: 1.0,
                    diameter: top_diameter,
                    color: theme::WARN,
                    label: "RES",
                    bipolar: false,
                },
                cx,
            ));

        /* EQ column: kill dot on the outer side of each knob */
        let mut eq_col = gpui::div()
            .flex()
            .flex_none()
            .w(px(eq_row_width))
            .flex_col()
            .justify_between()
            .items_center()
            .py_1();
        for (band, label) in [
            (EqBand::High, "HI"),
            (EqBand::Mid, "MID"),
            (EqBand::Low, "LOW"),
        ] {
            let killed = d.eq_kill[band.index()];
            let kill_btn = {
                let el = gpui::div()
                    .id(SharedString::from(format!("kill-{:?}-{:?}", deck, band)))
                    .flex_none()
                    .size(px(8.))
                    .rounded(px(2.))
                    .border_1()
                    .border_color(if killed {
                        theme::DANGER
                    } else {
                        gpui::rgb(0x05060a)
                    })
                    .bg(if killed {
                        theme::DANGER
                    } else {
                        gpui::rgb(0x0d0f14)
                    })
                    .hover(|s| s.bg(theme::with_alpha(theme::DANGER, 0.4)));
                let state = cx.entity();
                el.on_click(move |_ev, _window, cx| {
                    let on = state.read(cx).deck(deck).eq_kill[band.index()];
                    state.update(cx, |s, cx| {
                        s.set_kill(deck, band, !on);
                        cx.notify();
                    });
                })
            };

            let eq_row = gpui::div()
                .flex()
                .flex_none()
                .items_center()
                .w(px(eq_row_width))
                .when(flip, |el| el.flex_row_reverse())
                .child(kill_btn)
                .child(gpui::div().flex_1().flex().justify_center().child(knob(
                    KnobSpec {
                        ctl: KnobCtl::Eq(deck, band),
                        value: d.eq_db[band.index()],
                        min: -12.0,
                        max: 12.0,
                        diameter: eq_diameter,
                        color: if killed {
                            gpui::rgb(0x3a3a40)
                        } else {
                            theme::POINTER
                        },
                        label,
                        bipolar: true,
                    },
                    cx,
                )));
            eq_col = eq_col.child(eq_row);
        }

        let pfl_btn = {
            let on = d.pfl;
            let el = gpui::div()
                .id(SharedString::from(format!("pfl-{:?}", deck)))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(38.))
                .h(px(if compact_height { 18.0 } else { 20.0 }))
                .rounded(px(4.))
                .border_1()
                .border_color(if on {
                    theme::with_alpha(theme::LED_GREEN, 0.55)
                } else {
                    theme::LINE.into()
                })
                .bg(if on {
                    theme::with_alpha(theme::LED_GREEN, 0.16)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(8.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if on { theme::LED_GREEN } else { theme::MUTED })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .when(!pfl_available, |el| el.opacity(0.35))
                .child("PFL");
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                let on = state.read(cx).deck(deck).pfl;
                state.update(cx, |s, cx| {
                    s.set_pfl(deck, !on);
                    cx.notify();
                });
            })
        };

        /* One stereo meter pair per channel, beside the full-height fader. */
        let fader_row = gpui::div()
            .flex()
            .when(flip, |el| el.flex_row_reverse())
            .flex_1()
            .min_h_0()
            .justify_center()
            .gap(px(4.))
            .pb_1()
            .child(meter(d.level, 5.0))
            .child(fader_v(
                FaderSpec {
                    ctl: FaderCtl::Channel(deck),
                    value: d.fader,
                    min: 0.0,
                    max: 1.0,
                    width: 12.0,
                    color: dc,
                    ticks: true,
                },
                cx,
            ));

        gpui::div()
            .id(SharedString::from(format!("ch-{:?}", deck)))
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .child(
                gpui::div()
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(dc)
                    .child(deck_label(deck)),
            )
            .child(top_knobs)
            .child(
                gpui::div()
                    .flex()
                    .flex_1()
                    .min_h(px(154.))
                    .w_full()
                    .gap_2()
                    .when(flip, |el| el.flex_row_reverse())
                    .child(eq_col)
                    .child(
                        gpui::div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_2()
                            .items_center()
                            .child(fader_row)
                            .child(pfl_btn),
                    ),
            )
            .into_any_element()
    }
}
