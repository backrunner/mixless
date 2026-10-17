//! Track identity, remaining time, sounding BPM and key.

use gpui::{IntoElement, SharedString, prelude::*, px};
use mixless_protocol::{DeckId, DeckSnapshot};

use crate::{state::UiState, theme};

use super::deck_label;

impl UiState {
    pub(super) fn render_deck_header(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        d: &DeckSnapshot,
    ) -> gpui::AnyElement {
        let dc = theme::deck_color(deck);
        let loading = self.deck_loading[deck.index()].as_ref();
        let track = d
            .track_id
            .and_then(|id| self.tracks.iter().find(|t| t.id == id));
        let key = track
            .and_then(|t| t.camelot.as_deref().or(t.key.as_deref()))
            .map(|key| sounding_key(key, d));

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
                self.deck_time(deck, "deck", cx)
                    .text_size(px(15.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(if d.frames > 0 { dc } else { theme::MUTED })
                    .min_w(px(52.))
                    .flex()
                    .justify_end(),
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
            let key_color = theme::key_color(key.split_whitespace().next().unwrap_or(&key));
            header_right = header_right.child(
                gpui::div()
                    .flex_none()
                    .px_2()
                    .rounded(px(8.))
                    .bg(theme::with_alpha(key_color, 0.10))
                    .border_1()
                    .border_color(theme::with_alpha(key_color, 0.30))
                    .text_size(px(10.))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(key_color)
                    .child(key),
            );
        }

        gpui::div()
            .flex()
            .items_center()
            .gap_2()
            .flex_none()
            .child(focus_btn)
            .when(self.automix_active && d.automix_cue_frame.is_some(), |el| {
                el.child(
                    gpui::div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_size(px(8.))
                        .text_color(theme::WARN)
                        .child(if d.playing { "AUTO" } else { "AUTO CUE" }),
                )
            })
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
                            .truncate()
                            .child(
                                loading
                                    .cloned()
                                    .or_else(|| d.title.clone())
                                    .unwrap_or_default(),
                            ),
                    )
                    .child(
                        gpui::div()
                            .text_size(px(10.))
                            .text_color(if loading.is_some() {
                                theme::WARN
                            } else {
                                theme::MUTED
                            })
                            .truncate()
                            .child(if loading.is_some() {
                                if self.pending_play[deck.index()] {
                                    "Loading audio… will play".into()
                                } else {
                                    "Loading audio…".into()
                                }
                            } else if self.grid_pending(deck) {
                                "Ready to play, analyzing beats & phrases…".into()
                            } else {
                                d.artist.clone().unwrap_or_default()
                            }),
                    ),
            )
            .child(header_right)
            .into_any_element()
    }
}

pub(super) fn sounding_key(key: &str, deck: &DeckSnapshot) -> String {
    let pitch = deck.pitch_semitones
        + if deck.keylock {
            0.
        } else {
            12. * deck.rate.max(0.001).log2()
        };
    let Some(shifted) = mixless_protocol::transpose_camelot(key, pitch.round()) else {
        return if pitch.abs() < 0.005 {
            key.into()
        } else {
            format!("{key} {pitch:+.1} st")
        };
    };
    let cents = ((pitch - pitch.round()) * 100.).round() as i32;
    if cents == 0 {
        shifted
    } else {
        format!("{shifted} {cents:+}¢")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn badge_tracks_pitch_and_unlocked_tempo() {
        let mut d = DeckSnapshot::default();
        d.keylock = true;
        d.pitch_semitones = -3.;
        d.rate = 1.12;
        assert_eq!(sounding_key("7B", &d), "10B");
        d.keylock = false;
        d.rate = 2f32.powf(1. / 12.);
        assert_eq!(sounding_key("7B", &d), "5B");
        d.keylock = true;
        d.pitch_semitones = 0.25;
        assert_eq!(sounding_key("10A", &d), "10A +25¢");
    }
}
