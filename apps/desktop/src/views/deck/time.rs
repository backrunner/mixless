//! One time mode per deck, shared by its header and both waveform layouts.
use super::*;

impl UiState {
    pub(super) fn deck_time(
        &self,
        deck: DeckId,
        surface: &'static str,
        cx: &gpui::Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let d = self.deck(deck);
        let sr = if d.src_sample_rate > 0 {
            d.src_sample_rate
        } else {
            self.snapshot.sample_rate
        };
        let frame = (self.presentation_frames[deck.index()].max(0.) as u64).min(d.frames);
        let elapsed = self.deck_elapsed[deck.index()];
        let time = if elapsed {
            fmt_time(frame, sr)
        } else {
            format!("-{}", fmt_time(d.frames.saturating_sub(frame), sr))
        };
        gpui::div()
            .id(SharedString::from(format!("{surface}-time-{deck:?}")))
            .debug_selector(move || format!("{surface}-time-{deck:?}"))
            .cursor_pointer()
            .rounded(px(3.))
            .hover(|s| s.bg(theme::PANEL_RAISED))
            .tooltip(move |_, cx| {
                cx.new(|_| {
                    crate::views::TextTip(
                        if elapsed {
                            "Show remaining time"
                        } else {
                            "Show elapsed time"
                        }
                        .into(),
                    )
                })
                .into()
            })
            .child(time)
            .on_click(cx.listener(move |s, _, _, cx| {
                s.toggle_deck_time(deck);
                cx.stop_propagation();
                cx.notify();
            }))
    }
}
