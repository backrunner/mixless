//! FX catalogue and parameter editor overlay.

use mixless_protocol::{DeckId, FxKind};

use super::FxParam;
use crate::state::UiState;
use crate::{
    controls::{KnobSpec, knob},
    state::KnobCtl,
    theme,
};
use gpui::{Context, IntoElement, MouseButton, SharedString, div, prelude::*, px};

impl UiState {
    pub fn render_fx_editor(
        &self,
        deck: DeckId,
        slot: usize,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let fx = self.fx[deck.index()][slot];
        let color = theme::deck_color(deck);
        let mut catalogue = div()
            .id("fx-catalogue")
            .flex_1()
            .min_w_0()
            .h(px(420.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(2.));
        for family in [
            "Echo & Delay",
            "Reverb",
            "Modulation",
            "Filter",
            "Rhythm & Loop",
            "Pitch & Spectral",
            "Texture & Dynamics",
            "Noise & Sweep",
            "Release",
            "Macro",
        ] {
            let mut group = div().flex().flex_none().flex_wrap().gap(px(2.));
            for kind in FxKind::ALL
                .into_iter()
                .filter(|kind| kind.family() == family)
            {
                group = group.child(
                    div()
                        .id(SharedString::from(format!("choose-fx-{}", kind.as_str())))
                        .w(px(150.))
                        .flex()
                        .items_center()
                        .h(px(theme::MENU_ROW_HEIGHT))
                        .px(px(6.))
                        .rounded(px(2.))
                        .when(kind == fx.kind, |el| {
                            el.bg(theme::PANEL_RAISED).text_color(color)
                        })
                        .text_size(px(11.))
                        .cursor_pointer()
                        .hover(|s| s.bg(theme::PANEL_RAISED))
                        .child(kind.label())
                        .on_click(cx.listener(move |s, _, _, cx| {
                            s.select_fx(deck, slot, kind);
                            cx.notify();
                        })),
                );
            }
            catalogue = catalogue
                .child(
                    div()
                        .flex_none()
                        .mt(px(8.))
                        .mb(px(2.))
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child(family),
                )
                .child(group);
        }
        let mut beats = div().flex().flex_wrap().gap_1();
        for (value, label) in mixless_protocol::FX_BEATS
            .into_iter()
            .zip(mixless_protocol::FX_BEAT_LABELS)
        {
            if value > fx.kind.max_beats() {
                continue;
            }
            beats = beats.child(
                div()
                    .id(SharedString::from(format!("fx-beat-{label}")))
                    .px_2()
                    .py_1()
                    .rounded(px(3.))
                    .bg(if fx.beats == value {
                        theme::PANEL_RAISED
                    } else {
                        theme::PANEL_INSET
                    })
                    .text_color(if fx.beats == value {
                        color
                    } else {
                        theme::MUTED
                    })
                    .text_size(px(11.))
                    .cursor_pointer()
                    .child(label)
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.set_fx_beats(deck, slot, value);
                        cx.notify();
                    })),
            );
        }
        let mut knobs = div().flex().flex_wrap().gap_3();
        knobs = knobs.child(knob(
            KnobSpec {
                ctl: KnobCtl::FxMix(deck, slot),
                value: fx.mix,
                min: 0.,
                max: 1.,
                diameter: 36.,
                color,
                label: "MIX",
                bipolar: false,
            },
            cx,
        ));
        for param in FxParam::for_kind(fx.kind) {
            if matches!(param, FxParam::Rate) && fx.rate_hz == 0.0 {
                continue;
            }
            knobs = knobs.child(
                div()
                    .w(px(74.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .child(knob(
                        KnobSpec {
                            ctl: KnobCtl::FxParam(deck, slot, param),
                            value: param.value(&fx),
                            min: 0.,
                            max: 1.,
                            diameter: 36.,
                            color,
                            label: param.label(fx.kind),
                            bipolar: false,
                        },
                        cx,
                    ))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(theme::MUTED)
                            .child(param.display(&fx)),
                    ),
            );
        }
        let timing = fx.kind.is_modulated()
            || fx.kind.is_delay()
            || fx.kind.is_capture()
            || fx.kind.is_release()
            || fx.kind == FxKind::FreezeVerb
            || matches!(
                fx.kind,
                FxKind::Jet | FxKind::Mobius | FxKind::MobiusTriangle
            );
        let controls = div()
            .w(px(280.))
            .flex_none()
            .flex()
            .flex_col()
            .gap_2()
            .pl_3()
            .border_l_1()
            .border_color(theme::LINE)
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(color)
                    .child(fx.kind.label()),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::MUTED)
                    .child(fx.kind.description()),
            )
            .child(
                div()
                    .id("fx-editor-toggle")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(56.))
                    .h(px(theme::MENU_ROW_HEIGHT))
                    .text_size(px(11.))
                    .rounded(px(2.))
                    .bg(theme::PANEL_RAISED)
                    .text_color(if fx.on { color } else { theme::MUTED })
                    .cursor_pointer()
                    .child(if fx.on { "ON" } else { "OFF" })
                    .on_click(cx.listener(move |s, _, _, cx| {
                        s.toggle_fx(deck, slot);
                        cx.notify();
                    })),
            )
            .when(timing, |el| {
                el.child(
                    div()
                        .text_size(px(10.))
                        .text_color(theme::MUTED)
                        .child("BEATS"),
                )
                .child(beats)
            })
            .when(
                fx.kind.is_modulated()
                    || matches!(
                        fx.kind,
                        FxKind::Jet | FxKind::Mobius | FxKind::MobiusTriangle
                    ),
                |el| {
                    el.child(
                        div()
                            .id("fx-editor-sync")
                            .text_size(px(11.))
                            .cursor_pointer()
                            .text_color(color)
                            .child(if fx.rate_hz == 0.0 {
                                "SYNC · click for free rate"
                            } else {
                                "FREE · click for beat sync"
                            })
                            .on_click(cx.listener(move |s, _, _, cx| {
                                s.set_fx_sync(deck, slot, fx.rate_hz != 0.0);
                                cx.notify();
                            })),
                    )
                },
            )
            .child(knobs);
        let panel = div()
            .id("fx-editor-panel")
            .w(px(820.))
            .p_3()
            .rounded(px(theme::DIALOG_RADIUS))
            .bg(theme::PANEL)
            .border_1()
            .border_color(theme::LINE)
            .flex()
            .flex_col()
            .gap_3()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .pb_2()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(
                        div()
                            .text_size(px(13.))
                            .child(format!("Deck {deck:?} · FX {}", slot + 1)),
                    )
                    .child(
                        div()
                            .id("fx-editor-close")
                            .px_2()
                            .cursor_pointer()
                            .child("×")
                            .on_click(cx.listener(|s, _, _, cx| {
                                s.close_fx_editor();
                                cx.notify();
                            })),
                    ),
            )
            .child(div().flex().gap_4().child(catalogue).child(controls));
        div()
            .id("fx-editor-backdrop")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::rgba(0x000000b8))
            .on_click(cx.listener(|s, _, _, cx| {
                s.close_fx_editor();
                cx.notify();
            }))
            .child(panel)
            .into_any_element()
    }
}
