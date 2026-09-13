//! Playback, hot-cue pads and loop controls in the deck performance row.

use gpui::{IntoElement, SharedString, prelude::*, px};
use mixless_protocol::{CueKind, DeckId, DeckSnapshot};

use crate::{
    state::{TransportButton, UiState},
    theme,
};

use crate::controls::caption;
use gpui::{MouseButton, MouseDownEvent};

const LOOP_SIZES: [f32; 11] = [0.0625, 0.125, 0.25, 0.5, 1., 2., 4., 8., 16., 32., 64.];
const PERFORM_HEIGHT: f32 = 64.0;
const TRANSPORT_ICON: f32 = 21.0 * 0.85 * 0.85;

impl UiState {
    pub(super) fn render_deck_performance(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        d: &DeckSnapshot,
        inner_gap: f32,
    ) -> gpui::AnyElement {
        let dc = theme::deck_color(deck);
        let flip = deck == DeckId::B;
        let play_btn = {
            let el = gpui::div()
                .id(SharedString::from(format!("play-{:?}", deck)))
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .h(px(34.))
                .rounded(px(4.))
                .border_1()
                .border_color(if d.playing {
                    theme::with_alpha(theme::LED_GREEN, 0.55)
                } else {
                    theme::LINE.into()
                })
                .bg(if d.playing {
                    theme::with_alpha(theme::LED_GREEN, 0.16)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_color(if d.playing { theme::LED_GREEN } else { dc })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(transport_icon(d.playing, d.brake, dc));
            let state = cx.entity();
            el.on_mouse_down(MouseButton::Left, move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.begin_transport_press(deck, TransportButton::Play);
                    cx.notify();
                });
            })
        };
        let cue_btn = {
            let el = gpui::div()
                .id(SharedString::from(format!("cue-{:?}", deck)))
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .h(px(34.))
                .rounded(px(4.))
                .border_1()
                .border_color(theme::with_alpha(theme::DANGER, 0.45))
                .bg(theme::PANEL_INSET)
                .text_size(px(10.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(theme::DANGER)
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(if d.cue_previewing {
                    "PREVIEW"
                } else if d.temporary_cue_frame.is_some() {
                    "CUE ●"
                } else {
                    "CUE"
                });
            let state = cx.entity();
            el.on_mouse_down(MouseButton::Left, move |ev, _window, cx| {
                state.update(cx, |s, cx| {
                    if ev.modifiers.shift {
                        s.temporary_cue(deck, true);
                    } else {
                        s.begin_transport_press(deck, TransportButton::Cue);
                    }
                    cx.notify();
                });
            })
        };

        // Give the transport and pad grid the exact same row box. The
        // transport row is anchored to its bottom, so PLAY/CUE share a
        // baseline with pads 5–8 at every deck width.
        let mut cue_use = gpui::div()
            .flex()
            .items_center()
            .gap(px(3.))
            .h(px(26.))
            .mb_1();
        if let Some((index, id)) =
            self.cue_role_editor[deck.index()].filter(|(_, id)| Some(*id) == d.track_id)
        {
            let _ = id;
            cue_use = cue_use.child(
                gpui::div()
                    .text_size(px(10.))
                    .text_color(theme::cue_color(index))
                    .child(format!("{}", index + 1)),
            );
            for (kind, label) in [
                (CueKind::Hot, "AUTO"),
                (CueKind::In, "IN"),
                (CueKind::Out, "OUT"),
            ] {
                let state = cx.entity();
                let selected = d.cue_kinds[index] == kind;
                cue_use = cue_use.child(
                    gpui::div()
                        .id(SharedString::from(format!("cue-use-{deck:?}-{label}")))
                        .flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .h(px(22.))
                        .rounded(px(3.))
                        .border_1()
                        .border_color(if selected {
                            theme::cue_color(index)
                        } else {
                            theme::LINE
                        })
                        .bg(theme::PANEL_INSET)
                        .text_size(px(8.))
                        .text_color(theme::TEXT)
                        .child(label)
                        .hover(|s| s.bg(theme::PANEL_RAISED))
                        .on_click(move |_, _, cx| {
                            state.update(cx, |s, cx| {
                                s.assign_cue_role(deck, kind);
                                cx.notify();
                            });
                        }),
                );
            }
        } else if self.cue_shift[deck.index()] {
            cue_use = cue_use.child(
                gpui::div()
                    .text_size(px(9.))
                    .text_color(theme::MUTED)
                    .child("Select cue · AUTO / IN / OUT"),
            );
        }
        let transport = gpui::div()
            .flex()
            .flex_col()
            .flex_none()
            .justify_end()
            .w(px(132.))
            .h(px(PERFORM_HEIGHT))
            .child(cue_use)
            .child(
                gpui::div()
                    .flex()
                    .gap_1()
                    .when(flip, |el| el.flex_row_reverse())
                    .child(play_btn)
                    .child(cue_btn),
            );

        let pad_state = cx.entity();
        let build_pad_row = |row_i: usize| -> gpui::Div {
            let mut row_el = gpui::div().flex().flex_1().gap_1().min_h_0();
            for col in 0..4 {
                let i = row_i * 4 + col;
                let set = d.cues[i].is_some();
                let color = theme::cue_color(i);
                let pad = {
                    let el = gpui::div()
                        .id(SharedString::from(format!("pad-{:?}-{i}", deck)))
                        .flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .h(px(30.))
                        .rounded(px(4.))
                        .border_1()
                        .text_size(px(11.))
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(match d.cue_kinds[i] {
                            CueKind::In if set => format!("{} IN", i + 1),
                            CueKind::Out if set => format!("{} OUT", i + 1),
                            _ => (i + 1).to_string(),
                        })
                        .hover(|s| s.bg(theme::with_alpha(color, 0.15)));
                    if set {
                        el.bg(color)
                            .border_color(theme::with_alpha(color, 0.7))
                            .text_color(gpui::rgb(0x101216))
                    } else {
                        el.bg(theme::PANEL_INSET)
                            .border_color(theme::LINE)
                            .text_color(theme::with_alpha(color, 0.55))
                    }
                };
                let state = pad_state.clone();
                let pad = pad.on_mouse_down(
                    MouseButton::Left,
                    move |ev: &MouseDownEvent, _window, cx| {
                        state.update(cx, |s, cx| {
                            s.begin_pad_press(deck, i, ev.modifiers.shift);
                            cx.notify();
                        });
                    },
                );
                let state = pad_state.clone();
                let pad = pad.on_mouse_down(MouseButton::Right, move |_, _, cx| {
                    state.update(cx, |s, cx| {
                        s.clear_cue(deck, i);
                        cx.notify();
                    });
                });
                row_el = row_el.child(pad);
            }
            row_el
        };
        let pads = gpui::div()
            .flex()
            .flex_1()
            .min_w_0()
            .flex_col()
            .h(px(PERFORM_HEIGHT))
            .gap_1()
            .child(build_pad_row(0))
            .child(build_pad_row(1));

        let loop_step = |dir: i32| {
            let el = gpui::div()
                .id(SharedString::from(format!("loop-{dir}-{:?}", deck)))
                .flex()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h(px(18.))
                .rounded(px(3.))
                .border_1()
                .border_color(theme::LINE)
                .bg(theme::PANEL_INSET)
                .text_size(px(8.))
                .text_color(theme::MUTED)
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(if dir > 0 { "▲" } else { "▼" });
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    let (bars, on) = {
                        let d = s.deck(deck);
                        let i = LOOP_SIZES
                            .iter()
                            .position(|&b| b == d.loop_beats)
                            .unwrap_or(2);
                        let next = LOOP_SIZES
                            [((i as i32 + dir).clamp(0, LOOP_SIZES.len() as i32 - 1)) as usize];
                        (next, d.loop_on)
                    };
                    s.set_loop(deck, bars, on);
                    cx.notify();
                });
            })
        };
        let loop_toggle = {
            let el = gpui::div()
                .id(SharedString::from(format!("loop-size-{:?}", deck)))
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .w(px(46.))
                .rounded(px(3.))
                .border_1()
                .border_color(if d.loop_on {
                    theme::with_alpha(theme::LED_RED, 0.65)
                } else {
                    theme::LINE.into()
                })
                .bg(if d.loop_on {
                    theme::with_alpha(theme::LED_RED, 0.20)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(12.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if d.loop_on {
                    theme::LED_RED
                } else {
                    theme::TEXT
                })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(if d.loop_beats < 1. {
                    format!("1/{:.0}", 1. / d.loop_beats)
                } else {
                    format!("{:.0}", d.loop_beats)
                });
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    let bars = s.deck(deck).loop_beats;
                    let on = !s.deck(deck).loop_on;
                    s.set_loop(deck, bars, on);
                    cx.notify();
                });
            })
        };

        let loop_col = gpui::div()
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap(px(2.))
            .w(px(46.))
            .child(loop_step(1))
            .child(loop_toggle.flex_1())
            .child(loop_step(-1))
            .child(caption("BEATS"));

        gpui::div()
            .flex()
            .flex_none()
            .gap(px(inner_gap))
            .h(px(PERFORM_HEIGHT))
            .when(flip, |el| el.flex_row_reverse())
            .child(transport)
            .child(pads)
            .child(loop_col)
            .into_any_element()
    }
}

// Draw both states in an identical box, independent of font glyph metrics.
fn transport_icon(playing: bool, brake: bool, dc: gpui::Rgba) -> impl IntoElement {
    gpui::canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let x = f32::from(bounds.origin.x);
            let y = f32::from(bounds.origin.y);
            let size = TRANSPORT_ICON;
            let color = if playing { theme::LED_GREEN } else { dc };
            if playing && !brake {
                crate::controls::quad_fill(window, x, y, size, size, color);
            } else {
                let mut path = gpui::PathBuilder::fill();
                for (i, (dx, dy)) in (if brake {
                    [(0., 0.), (size, size), (0., size)]
                } else {
                    [(0., 0.), (size, size * 0.5), (0., size)]
                })
                .into_iter()
                .enumerate()
                {
                    let p = gpui::point(px(x + dx), px(y + dy));
                    if i == 0 {
                        path.move_to(p);
                    } else {
                        path.line_to(p);
                    }
                }
                path.close();
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        },
    )
    .flex_none()
    .size(px(TRANSPORT_ICON))
}
