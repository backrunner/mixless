//! Sync, key lock, pitch offset and the tempo fader.

use gpui::{IntoElement, SharedString, prelude::*, px};
use mixless_protocol::{DeckId, DeckSnapshot};

use crate::{state::UiState, theme};

use crate::{
    controls::{FaderSpec, KnobSpec, fader_v, knob},
    state::{FaderCtl, KnobCtl},
};

impl UiState {
    pub(super) fn render_deck_tempo(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        d: &DeckSnapshot,
    ) -> gpui::AnyElement {
        let dc = theme::deck_color(deck);
        let flip = deck == DeckId::B;
        let sync_btn = {
            let el = gpui::div()
                .id(SharedString::from(format!("sync-{:?}", deck)))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .w(px(50.))
                .h(px(22.))
                .rounded(px(4.))
                .border_1()
                .border_color(if d.synced || self.sync_waiting(deck) {
                    theme::with_alpha(theme::LED_GREEN, 0.55)
                } else {
                    theme::LINE.into()
                })
                .bg(if d.sync_locked {
                    theme::with_alpha(theme::LED_GREEN, 0.26)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if d.synced || self.sync_waiting(deck) {
                    theme::LED_GREEN
                } else {
                    theme::MUTED
                })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .cursor_pointer()
                .child(if self.sync_waiting(deck) {
                    "SYNC …"
                } else if d.sync_master {
                    "MASTER"
                } else if d.synced && !d.sync_locked {
                    "SYNC …"
                } else if d.frames > 0 && !d.grid_ready && self.grid_pending(deck) {
                    "GRID …"
                } else if d.frames > 0 && !d.grid_ready {
                    "NO GRID"
                } else {
                    "SYNC"
                });
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.sync(deck);
                    cx.notify();
                });
            })
        };

        let shift_btn = gpui::div()
            .id(SharedString::from(format!("shift-{deck:?}")))
            .flex()
            .items_center()
            .justify_center()
            .w(px(38.))
            .h(px(22.))
            .rounded(px(4.))
            .border_1()
            .border_color(if self.cue_shift[deck.index()] {
                dc
            } else {
                theme::LINE
            })
            .bg(if self.cue_shift[deck.index()] {
                theme::with_alpha(dc, 0.2)
            } else {
                theme::PANEL_INSET.into()
            })
            .text_color(if self.cue_shift[deck.index()] {
                dc
            } else {
                theme::MUTED
            })
            .text_size(px(8.))
            .font_weight(gpui::FontWeight::BOLD)
            .child("SHIFT")
            .on_click(cx.listener(move |state, _, _, cx| {
                state.toggle_cue_shift(deck);
                cx.notify();
            }));

        // SHIFT owns the full-height pitch lane. SYNC/key controls and the
        // optional stem stack occupy the adjacent column.
        let tempo_buttons = gpui::div()
            .flex()
            .flex_none()
            .flex_col()
            .gap(px(4.))
            .child(sync_btn)
            .child({
                let keylock = d.keylock;
                gpui::div()
                    .id(SharedString::from(format!("keylock-{deck:?}")))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .w(px(50.))
                    .h(px(18.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if keylock {
                        theme::with_alpha(dc, 0.55)
                    } else {
                        theme::LINE.into()
                    })
                    .bg(if keylock {
                        theme::with_alpha(dc, 0.14)
                    } else {
                        theme::PANEL_INSET.into()
                    })
                    .text_color(if keylock { dc } else { theme::MUTED })
                    .text_size(px(8.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("KEY LOCK")
                    .on_click(cx.listener(move |state, _, _, cx| {
                        state
                            .dispatch(mixless_protocol::Command::SetKeyLock { deck, on: !keylock });
                        cx.notify();
                    }))
            });
        let sync_column = gpui::div()
            .flex()
            .flex_none()
            .flex_col()
            .min_h_0()
            .items_start()
            .when(flip, |el| el.items_end())
            .gap(px(8.))
            .w(px(86.))
            .child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .h(px(44.))
                    .items_start()
                    .when(flip, |el| el.flex_row_reverse())
                    .gap(px(6.))
                    .child(tempo_buttons)
                    .child(knob(
                        KnobSpec {
                            ctl: KnobCtl::Key(deck),
                            value: d.pitch_semitones,
                            min: -6.0,
                            max: 6.0,
                            diameter: 22.0,
                            color: theme::POINTER,
                            label: "KEY",
                            bipolar: true,
                        },
                        cx,
                    )),
            )
            .children(self.render_stems(cx, deck, d));
        let pitch_column = gpui::div()
            .flex()
            .flex_none()
            .flex_col()
            .min_h_0()
            .items_center()
            .gap(px(8.))
            .w(px(38.))
            .child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .flex_col()
                    .h(px(44.))
                    .items_center()
                    .justify_between()
                    .child(shift_btn.flex_none())
                    .child(
                        gpui::div()
                            .text_size(px(9.))
                            .text_color(theme::MUTED)
                            .child(format!("{:+.1}%", (d.rate - 1.0) * 100.0)),
                    ),
            )
            .child(
                fader_v(
                    FaderSpec {
                        ctl: FaderCtl::Tempo(deck),
                        value: d.rate,
                        min: 0.88,
                        max: 1.12,
                        width: 6.0,
                        color: theme::POINTER,
                        ticks: true,
                    },
                    cx,
                )
                // Keep the grab area wide even though the visible rail is slim.
                .w(px(38.))
                .flex_1()
                .min_h_0(),
            );
        gpui::div()
            .flex()
            .flex_none()
            .min_h_0()
            .gap(px(4.))
            .when(flip, |el| el.flex_row_reverse())
            .child(pitch_column)
            .child(sync_column)
            .into_any_element()
    }
}
