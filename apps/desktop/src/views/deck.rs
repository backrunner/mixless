//! Deck panels: track header, tempo section (rekordbox-style: SYNC + key +
//! full-height pitch fader on the outer edge), jog wheel, transport, cue
//! pads, loop controls. Also the FX bar, aligned to the deck/mixer/deck grid.

use std::sync::Arc;

use gpui::prelude::*;
use gpui::{IntoElement, MouseButton, MouseDownEvent, SharedString, px};
use mixless_protocol::{DeckId, Waveform};

use crate::controls::{FaderSpec, JogSpec, KnobSpec, caption, fader_v, jog, knob};
use crate::state::{FaderCtl, KnobCtl, UiState};
use crate::theme;
use crate::views::library::TrackDrag;

const LOOP_SIZES: [u16; 5] = [1, 2, 4, 8, 16];
const PERFORM_HEIGHT: f32 = 64.0;

fn fmt_time(frames: u64, sr: u32) -> String {
    if sr == 0 {
        return "0:00".into();
    }
    let sec = frames / sr as u64;
    format!("{}:{:02}", sec / 60, sec % 60)
}

pub fn deck_label(deck: DeckId) -> &'static str {
    match deck {
        DeckId::A => "A",
        DeckId::B => "B",
    }
}

impl UiState {
    pub fn render_wave_strip(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        height: f32,
    ) -> gpui::AnyElement {
        let d = self.deck(deck).clone();
        let wave = self.wave[deck.index()].clone().map(|(_, w)| w);
        let track = d
            .track_id
            .and_then(|id| self.tracks.iter().find(|t| t.id == id));
        let device_sr = self.snapshot.sample_rate;
        let dc = theme::deck_color(deck);
        let loaded = d.frames > 0;
        let sr = if d.src_sample_rate > 0 {
            d.src_sample_rate
        } else {
            device_sr
        };
        let remain = if loaded && sr > 0 {
            format!("-{}", fmt_time(d.frames.saturating_sub(d.frame), sr))
        } else {
            "-0:00".into()
        };
        let initial = (d.title.as_deref().unwrap_or("♪"))
            .trim()
            .chars()
            .next()
            .map(|c| c.to_uppercase().next().unwrap_or('♪'))
            .unwrap_or('♪');

        let info = gpui::div()
            .flex()
            .items_center()
            .gap_2()
            .w(px(240.))
            .flex_none()
            .child(
                gpui::div()
                    .flex_none()
                    .size(px(34.))
                    .rounded(px(5.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(if loaded { dc } else { theme::MUTED })
                    .border_1()
                    .border_color(if loaded {
                        theme::with_alpha(dc, 0.35)
                    } else {
                        theme::LINE.into()
                    })
                    .bg(theme::PANEL_INSET)
                    .child(initial.to_string()),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        gpui::div()
                            .text_size(px(12.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT)
                            .overflow_hidden()
                            .child(d.title.clone().unwrap_or_else(|| "— empty —".into())),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(10.))
                            .text_color(theme::MUTED)
                            .overflow_hidden()
                            .child(d.artist.clone().unwrap_or_default()),
                    ),
            )
            .child(
                gpui::div()
                    .flex()
                    .flex_none()
                    .flex_col()
                    .items_end()
                    .gap(px(1.))
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(
                        gpui::div()
                            .text_size(px(12.))
                            .text_color(theme::TEXT)
                            .child(
                                track
                                    .and_then(|t| t.bpm)
                                    .map(|b| format!("{b:.1}"))
                                    .unwrap_or_else(|| "—".into()),
                            ),
                    )
                    .child(
                        gpui::div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::ACCENT)
                            .child(
                                track
                                    .and_then(|t| t.camelot.clone().or_else(|| t.key.clone()))
                                    .unwrap_or_default(),
                            ),
                    )
                    .child(remain),
            );

        gpui::div()
            .flex()
            .flex_none()
            .h(px(height))
            .gap_3()
            .child(info)
            .child(
                crate::wave::wave_lane(
                    SharedString::from(format!("wave-strip-{:?}", deck)),
                    false,
                    deck,
                    d,
                    wave,
                    self.wave_tempo[deck.index()].clone(),
                    device_sr,
                    dc,
                    cx,
                )
                .into_any_element(),
            )
            .into_any_element()
    }

    pub fn render_vertical_wave(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
    ) -> gpui::AnyElement {
        let d = self.deck(deck).clone();
        let wave: Option<Arc<Waveform>> = self.wave[deck.index()].clone().map(|(_, w)| w);
        let device_sr = self.snapshot.sample_rate;
        let dc = theme::deck_color(deck);
        let sr = if d.src_sample_rate > 0 {
            d.src_sample_rate
        } else {
            device_sr
        };
        let loaded = d.frames > 0;
        let remain = if loaded && sr > 0 {
            format!("-{}", fmt_time(d.frames.saturating_sub(d.frame), sr))
        } else {
            "-0:00".into()
        };

        gpui::div()
            .w_full()
            .flex()
            .flex_col()
            .items_center()
            .gap_1()
            .min_h_0()
            .child(
                gpui::div()
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(if loaded { dc } else { theme::MUTED })
                    .child(deck_label(deck)),
            )
            .child(
                crate::wave::wave_lane(
                    SharedString::from(format!("vwave-{:?}", deck)),
                    true,
                    deck,
                    d,
                    wave,
                    self.wave_tempo[deck.index()].clone(),
                    device_sr,
                    dc,
                    cx,
                )
                .into_any_element(),
            )
            .child(
                gpui::div()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .min_h(px(12.))
                    .child(remain),
            )
            .into_any_element()
    }

    pub fn render_deck(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        min_width: f32,
        compact_height: bool,
    ) -> gpui::AnyElement {
        let d = self.deck(deck).clone();
        let dc = theme::deck_color(deck);
        let flip = deck == DeckId::B;
        let focused = self.focus == deck;
        let sr = if d.src_sample_rate > 0 {
            d.src_sample_rate
        } else {
            self.snapshot.sample_rate
        };
        let track = d
            .track_id
            .and_then(|id| self.tracks.iter().find(|t| t.id == id));
        let key = track.and_then(|t| t.camelot.clone().or_else(|| t.key.clone()));
        let inner_gap = if compact_height { 4.0 } else { 8.0 };
        let panel_padding = if compact_height { 8.0 } else { 12.0 };
        let tempo_fader_min_height = if compact_height { 44.0 } else { 60.0 };

        let remain = if d.frames > 0 && sr > 0 {
            format!("-{}", fmt_time(d.frames.saturating_sub(d.frame), sr))
        } else {
            "-0:00".into()
        };

        /* header ---------------------------------------------------------------- */

        let focus_btn = {
            let el = gpui::div()
                .id(SharedString::from(format!("deck-tag-{:?}", deck)))
                .flex_none()
                .size(px(26.))
                .rounded(px(6.))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(13.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(dc)
                .border_1()
                .border_color(theme::with_alpha(dc, 0.65))
                .bg(theme::with_alpha(dc, 0.14))
                .child(deck_label(deck))
                .hover(|s| s.bg(theme::with_alpha(dc, 0.26)));
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.focus = deck;
                    cx.notify();
                });
            })
        };

        let mut header_right = gpui::div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .child(
                gpui::div()
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(if d.frames > 0 { dc } else { theme::MUTED })
                    .min_w(px(52.))
                    .flex()
                    .justify_end()
                    .child(remain),
            )
            .child(
                gpui::div()
                    .flex()
                    .items_baseline()
                    .gap_1()
                    .child(
                        gpui::div()
                            .text_size(px(17.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(theme::TEXT)
                            .min_w(px(50.))
                            .flex()
                            .justify_end()
                            .child(if d.sounding_bpm > 0.0 {
                                format!("{:.2}", d.sounding_bpm)
                            } else {
                                "—".into()
                            }),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(8.))
                            .text_color(theme::MUTED)
                            .child("BPM"),
                    ),
            );
        if let Some(key) = key {
            header_right = header_right.child(
                gpui::div()
                    .flex_none()
                    .px_2()
                    .rounded(px(8.))
                    .bg(theme::with_alpha(theme::ACCENT, 0.10))
                    .border_1()
                    .border_color(theme::with_alpha(theme::ACCENT, 0.30))
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(theme::ACCENT)
                    .child(key),
            );
        }

        let header = gpui::div()
            .flex()
            .items_center()
            .gap_2()
            .flex_none()
            .child(focus_btn)
            .child(
                gpui::div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        gpui::div()
                            .text_size(px(13.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(theme::TEXT)
                            .overflow_hidden()
                            .child(d.title.clone().unwrap_or_else(|| "— empty —".into())),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(10.))
                            .text_color(theme::MUTED)
                            .overflow_hidden()
                            .child(d.artist.clone().unwrap_or_default()),
                    ),
            )
            .child(header_right);

        /* tempo stack (rekordbox: SYNC on top, key knob, tall pitch fader) ------ */

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
                .border_color(if d.synced {
                    theme::with_alpha(theme::LED_GREEN, 0.55)
                } else {
                    theme::LINE.into()
                })
                .bg(if d.synced {
                    theme::with_alpha(theme::LED_GREEN, 0.16)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(9.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if d.synced {
                    theme::LED_GREEN
                } else {
                    theme::MUTED
                })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child("SYNC");
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.sync(deck);
                    cx.notify();
                });
            })
        };

        let tempo_stack = gpui::div()
            .flex()
            .flex_none()
            .flex_col()
            .items_center()
            .gap(px(inner_gap))
            .w(px(56.))
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
            })
            .child(knob(
                KnobSpec {
                    ctl: KnobCtl::Key(deck),
                    value: d.pitch_semitones,
                    min: -6.0,
                    max: 6.0,
                    diameter: 26.0,
                    color: theme::POINTER,
                    label: "KEY",
                    bipolar: true,
                },
                cx,
            ))
            .child(
                gpui::div()
                    .flex_none()
                    .text_size(px(10.))
                    .text_color(theme::MUTED)
                    .child(format!("{:+.1}%", (d.rate - 1.0) * 100.0)),
            )
            .child(
                fader_v(
                    FaderSpec {
                        ctl: FaderCtl::Tempo(deck),
                        value: d.rate,
                        min: 0.88,
                        max: 1.12,
                        width: 12.0,
                        color: theme::POINTER,
                        ticks: true,
                    },
                    cx,
                )
                .flex_1()
                .min_h(px(tempo_fader_min_height)),
            );

        /* center: the jog fills all remaining deck space ----------------------- */

        let initial = (d.title.as_deref().unwrap_or(deck_label(deck)))
            .trim()
            .chars()
            .next()
            .map(|c| c.to_uppercase().next().unwrap_or('A'))
            .unwrap_or('A');

        let center_col = gpui::div().flex().flex_1().min_w_0().min_h_0().child(jog(
            JogSpec {
                deck,
                frame: d.frame,
                frames: d.frames,
                src_sample_rate: d.src_sample_rate,
                playing: d.playing,
                color: dc,
                initial,
            },
            cx,
        ));

        let body = gpui::div()
            .flex()
            .flex_1()
            .min_h_0()
            .gap(px(inner_gap))
            .when(flip, |el| el.flex_row_reverse())
            .child(tempo_stack)
            .child(center_col);

        /* perform row: transport | pads | loop ----------------------------------- */

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
                .text_size(px(14.))
                .text_color(if d.playing { theme::LED_GREEN } else { dc })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(if d.playing { "❚❚" } else { "▶" });
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.play_pause(deck);
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
                .child("CUE");
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    s.jump_cue(deck, 0);
                    cx.notify();
                });
            })
        };

        // Give the transport and pad grid the exact same row box. The
        // transport row is anchored to its bottom, so PLAY/CUE share a
        // baseline with pads 5–8 at every deck width.
        let transport = gpui::div()
            .flex()
            .flex_col()
            .flex_none()
            .justify_end()
            .w(px(132.))
            .h(px(PERFORM_HEIGHT))
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
                        .child((i + 1).to_string())
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
                        if ev.modifiers.shift {
                            state.update(cx, |s, cx| {
                                s.set_cue_now(deck, i);
                                cx.notify();
                            });
                        } else {
                            state.update(cx, |s, cx| {
                                s.jump_cue(deck, i);
                                cx.notify();
                            });
                        }
                    },
                );
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
                            .position(|&b| b == d.loop_bars)
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
                    theme::with_alpha(theme::ACCENT, 0.55)
                } else {
                    theme::LINE.into()
                })
                .bg(if d.loop_on {
                    theme::with_alpha(theme::ACCENT, 0.16)
                } else {
                    theme::PANEL_INSET.into()
                })
                .text_size(px(12.))
                .font_weight(gpui::FontWeight::BOLD)
                .text_color(if d.loop_on {
                    theme::ACCENT
                } else {
                    theme::TEXT
                })
                .hover(|s| s.bg(theme::PANEL_RAISED))
                .child(d.loop_bars.to_string());
            let state = cx.entity();
            el.on_click(move |_ev, _window, cx| {
                state.update(cx, |s, cx| {
                    let bars = s.deck(deck).loop_bars;
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
            .child(caption("LOOP"));

        let perform = gpui::div()
            .flex()
            .flex_none()
            .gap(px(inner_gap))
            .h(px(PERFORM_HEIGHT))
            .when(flip, |el| el.flex_row_reverse())
            .child(transport)
            .child(pads)
            .child(loop_col);

        gpui::div()
            .id(SharedString::from(format!("deck-{:?}", deck)))
            .drag_over::<TrackDrag>(move |style, _, _, _| {
                style.bg(theme::with_alpha(dc, 0.12)).border_color(dc)
            })
            .on_drop(cx.listener(move |s, track: &TrackDrag, _, cx| {
                s.focus = deck;
                s.track_sel = Some(track.id.0);
                s.load_deck(deck, track.id);
                cx.notify();
            }))
            .flex()
            .flex_1()
            .min_w(px(min_width))
            .flex_shrink_0()
            .min_h_0()
            .flex_col()
            .gap(px(inner_gap))
            .p(px(panel_padding))
            .rounded(px(8.))
            .bg(theme::PANEL)
            .border_1()
            .border_color(if focused {
                theme::with_alpha(dc, 0.55)
            } else {
                theme::LINE.into()
            })
            .child(header)
            .child(body)
            .child(perform)
            .into_any_element()
    }

    /* ---- FX bar --------------------------------------------------------------- */

    fn fx_slot(&self, cx: &mut gpui::Context<Self>, deck: DeckId, slot: usize) -> gpui::AnyElement {
        let fx = self.fx[deck.index()][slot].clone();
        let on = fx.on;
        let held = self.momentary_fx_active(deck, slot);
        let active = on || held;
        let dc = theme::deck_color(deck);
        let kind: SharedString = fx.kind.into();

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
                .w(px(48.))
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
            .px_2()
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
                    .gap_2()
                    .when(deck == DeckId::B, |el| el.flex_row_reverse())
                    .child(mix)
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
            let mut ch_el = gpui::div().flex().flex_1().min_w_0().items_center().gap_2();
            ch_el = ch_el.child(
                gpui::div()
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(dc)
                    .child(deck_label(DeckId::A)),
            );
            for slot in 0..3 {
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
                .gap_2();
            ch_el = ch_el.child(
                gpui::div()
                    .flex_none()
                    .text_size(px(11.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(dc)
                    .child(deck_label(DeckId::B)),
            );
            for slot in 0..3 {
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
