use crate::{
    controls::{KnobSpec, knob},
    state::{KnobCtl, UiState},
    theme,
};
use gpui::{MouseButton, SharedString, prelude::*, px};
use mixless_protocol::{Command, DeckId, DeckSnapshot, StemKind};

impl UiState {
    pub(super) fn render_stems(
        &self,
        cx: &mut gpui::Context<Self>,
        deck: DeckId,
        d: &DeckSnapshot,
    ) -> Option<gpui::AnyElement> {
        if !d.stems_ready {
            return None;
        }
        let mut column = gpui::div()
            .flex()
            .flex_wrap()
            .flex_none()
            .size(px(76.))
            .content_start()
            .items_center()
            .justify_center()
            .gap(px(4.));
        for (stem, label, color) in [
            (StemKind::Vocals, "VOCAL", theme::WARN),
            (StemKind::Drums, "DRUMS", theme::DECK_A),
            (StemKind::Bass, "BASS", theme::DECK_B),
            (StemKind::Instruments, "OTHER", theme::MUTED),
        ] {
            if stem == StemKind::Bass && !d.bass_ready {
                column = column.child(
                    gpui::div()
                        .w(px(36.))
                        .h(px(36.))
                        .text_size(px(8.))
                        .text_color(theme::MUTED)
                        .child("BASS —"),
                );
                continue;
            }
            let value = d.stem_gain[stem.index()];
            let state = cx.entity();
            let toggle = gpui::div()
                .id(SharedString::from(format!("stem-{deck:?}-{stem:?}")))
                .w_full()
                .text_size(px(8.))
                .line_height(px(10.))
                .text_center()
                .text_color(if value > 0. { color } else { theme::MUTED })
                .cursor_pointer()
                .child(label)
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    state.update(cx, |s, cx| {
                        let current = s.deck(deck).stem_gain[stem.index()];
                        s.dispatch(Command::SetStemGain {
                            deck,
                            stem,
                            value: if current > 0. { 0. } else { 1. },
                        });
                        cx.notify();
                    });
                });
            column = column.child(
                gpui::div()
                    .flex()
                    .flex_col()
                    .flex_none()
                    .w(px(36.))
                    .h(px(36.))
                    .items_center()
                    .gap(px(2.))
                    .child(knob(
                        KnobSpec {
                            ctl: KnobCtl::Stem(deck, stem),
                            value,
                            min: 0.,
                            max: 1.,
                            diameter: 22.,
                            color,
                            label: "",
                            bipolar: false,
                        },
                        cx,
                    ))
                    .child(toggle),
            );
        }
        Some(column.into_any_element())
    }
}
