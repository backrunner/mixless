//! Deck panel composition and shared display helpers. Each child owns one
//! visual section; FX slot rendering lives with the FX editor.

mod header;
mod performance;
mod stems;
mod tempo;
mod wave;

use gpui::{IntoElement, SharedString, prelude::*, px};
use mixless_protocol::DeckId;

use crate::{
    controls::{JogSpec, jog},
    state::UiState,
    theme,
    views::library::TrackDrag,
};

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
        let inner_gap = if compact_height { 4.0 } else { 8.0 };
        let panel_padding = if compact_height { 8.0 } else { 12.0 };
        let header = self.render_deck_header(cx, deck, &d);
        let tempo_stack = self.render_deck_tempo(cx, deck, &d);

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
                frame: self.presentation_frames[deck.index()],
                frames: d.frames,
                src_sample_rate: d.src_sample_rate,
                playing: d.playing,
                color: dc,
                initial,
                artwork: self.deck_artwork[deck.index()].image.clone(),
                end_warning: if d.frames > 0
                    && d.playing
                    && !d.loop_on
                    && !d.reverse
                    && d.src_sample_rate > 0
                {
                    let remaining = d.frames.saturating_sub(d.frame) as f32
                        / d.src_sample_rate as f32
                        / d.rate.max(0.01);
                    (remaining <= 30.).then_some(remaining)
                } else {
                    None
                },
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

        let perform = self.render_deck_performance(cx, deck, &d, inner_gap);

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
}
