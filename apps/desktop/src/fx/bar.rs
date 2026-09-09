//! Mirrored deck FX slots, latch/hold controls and editor entry points.

use gpui::{IntoElement, MouseButton, MouseDownEvent, SharedString, prelude::*, px};
use mixless_protocol::DeckId;

use crate::{
    controls::{KnobSpec, knob},
    state::{KnobCtl, UiState},
    theme,
    views::deck::deck_label,
};

impl UiState {
    fn fx_slot(&self, cx: &mut gpui::Context<Self>, deck: DeckId, slot: usize) -> gpui::AnyElement {
        let fx = self.fx[deck.index()][slot].clone();
        let on = fx.on;
        let held = self.momentary_fx_active(deck, slot);
        let active = on || held;
        let dc = theme::deck_color(deck);
        let kind: SharedString = fx.kind.label().into();

        let arrow = |cx: &mut gpui::Context<Self>, dir: i32| {
            // B is a true horizontal mirror of A, including arrow direction,
            // rather than only reversing the child order.
            let arrow_glyph = match (deck, dir > 0) {
                (DeckId::A, true) | (DeckId::B, false) => "▶",
                (DeckId::A, false) | (DeckId::B, true) => "◀",
            };
            let el = gpui::div()
                .id(SharedString::from(format!("fx-{dir}-{:?}-{slot}", deck)))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(px(14.))
                .rounded(px(3.))
                .text_size(px(7.))
                .text_color(theme::MUTED)
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(arrow_glyph);
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.cycle_fx(deck, slot, dir);
                    cx.notify();
                });
            })
        };
        let kind_btn = {
            let state = cx.entity();
            let el = gpui::div()
                .id(SharedString::from(format!("fx-kind-{:?}-{slot}", deck)))
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .justify_center()
                .h(px(22.))
                .px_1()
                .rounded(px(4.))
                .border_1()
                .border_color(if active {
                    theme::with_alpha(dc, 0.65)
                } else {
                    theme::LINE.into()
                })
                .bg(if active {
                    theme::with_alpha(dc, if held { 0.30 } else { 0.16 })
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if active { dc } else { theme::MUTED })
                .overflow_hidden()
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(kind.clone());
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.toggle_fx(deck, slot);
                    cx.notify();
                });
            })
        };

        let hold = {
            let down = cx.listener(move |s: &mut UiState, _ev: &MouseDownEvent, window, cx| {
                window.prevent_default();
                s.begin_momentary_fx(deck, slot);
                cx.notify();
            });
            gpui::div()
                .id(SharedString::from(format!("fx-hold-{:?}-{slot}", deck)))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .w(px(40.))
                .h(px(22.))
                .rounded(px(4.))
                .border_1()
                .border_color(if held {
                    theme::with_alpha(theme::WARN, 0.75)
                } else {
                    theme::LINE.into()
                })
                .bg(if held {
                    theme::with_alpha(theme::WARN, 0.24)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(8.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if held { theme::WARN } else { theme::MUTED })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .on_mouse_down(MouseButton::Left, down)
                .child("HOLD")
        };

        let mix = gpui::div()
            .flex()
            .when(deck == DeckId::B, |el| el.flex_row_reverse())
            .flex_1()
            .min_w_0()
            .items_center()
            .gap_1()
            .child(
                gpui::div()
                    .flex_none()
                    .text_size(px(7.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::MUTED)
                    .child("MIX"),
            )
            .child(gpui::div().flex_none().child(knob(
                KnobSpec {
                    ctl: KnobCtl::FxMix(deck, slot),
                    value: fx.mix,
                    min: 0.0,
                    max: 1.0,
                    diameter: 18.0,
                    color: if active { dc } else { gpui::rgb(0x5a5a60) },
                    label: "",
                    bipolar: false,
                },
                cx,
            )));

        gpui::div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .gap_1()
            .h(px(60.))
            .px(px(6.))
            .py_1()
            .rounded(px(4.))
            .border_1()
            .border_color(if active {
                theme::with_alpha(dc, 0.50)
            } else {
                theme::LINE_SOFT.into()
            })
            .bg(if active {
                theme::with_alpha(gpui::rgb(0x0a0a0c), 0.9)
            } else {
                theme::PANEL_INSET.into()
            })
            .child(
                gpui::div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .when(deck == DeckId::B, |el| el.flex_row_reverse())
                    .child(arrow(cx, -1))
                    .child(kind_btn)
                    .child(arrow(cx, 1)),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .items_center()
                    .gap_1()
                    .when(deck == DeckId::B, |el| el.flex_row_reverse())
                    .child(mix)
                    .child(div_fx_edit(deck, slot, cx))
                    .child(hold),
            )
            .into_any_element()
    }

    pub fn render_fxbar(
        &self,
        cx: &mut gpui::Context<Self>,
        _center_width: f32,
    ) -> gpui::AnyElement {
        let ch_a = {
            let dc = theme::deck_color(DeckId::A);
            let mut ch_el = gpui::div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(px(6.));
            ch_el = ch_el.child(
                gpui::div()
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(dc)
                    .child(deck_label(DeckId::A)),
            );
            for slot in 0..self.fx[DeckId::A.index()].len() {
                ch_el = ch_el.child(self.fx_slot(cx, DeckId::A, slot));
            }
            ch_el.into_any_element()
        };
        let ch_b = {
            let dc = theme::deck_color(DeckId::B);
            let mut ch_el = gpui::div()
                .flex()
                .flex_1()
                .min_w_0()
                .flex_row_reverse()
                .items_center()
                .gap(px(6.));
            ch_el = ch_el.child(
                gpui::div()
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(dc)
                    .child(deck_label(DeckId::B)),
            );
            for slot in 0..self.fx[DeckId::B.index()].len() {
                ch_el = ch_el.child(self.fx_slot(cx, DeckId::B, slot));
            }
            ch_el.into_any_element()
        };

        gpui::div()
            .id("fxbar")
            .flex()
            .flex_none()
            .items_center()
            .h(px(68.))
            .gap_2()
            .px_2()
            .rounded(px(8.))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .child(ch_a)
            .child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .w(px(28.))
                    .justify_center()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::MUTED)
                    .child("FX"),
            )
            .child(ch_b)
            .into_any_element()
    }
}

fn div_fx_edit(deck: DeckId, slot: usize, cx: &mut gpui::Context<UiState>) -> gpui::AnyElement {
    gpui::div()
        .id(SharedString::from(format!("fx-edit-{deck:?}-{slot}")))
        .text_size(px(9.))
        .text_color(theme::MUTED)
        .cursor_pointer()
        .px_1()
        .child("⋯")
        .on_click(cx.listener(move |s, _, _, cx| {
            s.end_drag();
            s.end_momentary_fx();
            s.fx_editor = Some((deck, slot));
            cx.notify();
        }))
        .into_any_element()
}
