//! Horizontal track overview and vertical performance waveform lanes.

use gpui::{IntoElement, SharedString, prelude::*, px};
use mixless_protocol::DeckId;

use super::{deck_label, fmt_time};
use crate::{state::UiState, theme};

impl UiState {
    pub fn render_wave_strip(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        height: f32,
    ) -> gpui::AnyElement {
        let mut d = self.deck(deck).clone();
        d.frame = self.presentation_frames[deck.index()].round() as u64;
        let wave = self.wave_cache[deck.index()].clone();
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
                            .child(d.title.clone().unwrap_or_default()),
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
                    self.transition_window(deck),
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
        let mut d = self.deck(deck).clone();
        d.frame = self.presentation_frames[deck.index()].round() as u64;
        let wave = self.wave_cache[deck.index()].clone();
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
                    self.transition_window(deck),
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
}
