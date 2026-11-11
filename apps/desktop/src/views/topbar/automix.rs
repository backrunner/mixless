//! AutoMix-only mode and transport controls.
use super::*;
use crate::views::TextTip;
use gpui::{Stateful, div, svg};
use mixless_protocol::Command;

fn button(id: &'static str, active: bool, enabled: bool) -> Stateful<gpui::Div> {
    div()
        .id(id)
        .debug_selector(move || id.into())
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .h(px(24.))
        .rounded(px(5.))
        .border_1()
        .border_color(if active {
            theme::with_alpha(theme::ACCENT, 0.4)
        } else {
            theme::LINE.into()
        })
        .bg(if active {
            theme::with_alpha(theme::ACCENT, 0.08)
        } else {
            theme::PANEL_INSET.into()
        })
        .text_color(if active { theme::ACCENT } else { theme::TEXT })
        .when(!enabled, |el| el.opacity(0.4))
        .when(enabled, |el| {
            el.cursor_pointer().hover(|s| s.bg(theme::PANEL_RAISED))
        })
}

impl UiState {
    pub(super) fn render_automix_controls(&self, cx: &mut gpui::Context<Self>) -> gpui::Div {
        let mode = self.order_button_mode();
        let shuffle = mode == crate::automix::OrderMode::Shuffle;
        let best = mode == crate::automix::OrderMode::Best;
        let label = mode.label();
        let tip = "Choose playback order";
        let paused = self.snapshot.automix_paused;
        let enabled = self.snapshot.automix_on;
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(6.))
            .child(
                button("automix-order", shuffle || best, true)
                    .relative()
                    .child(self.order_menu.anchor_marker())
                    .px_2()
                    .gap(px(5.))
                    .text_size(px(8.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(label)
                    .child(
                        svg()
                            .path(if shuffle {
                                "icons/shuffle.svg"
                            } else {
                                "icons/repeat.svg"
                            })
                            .size(px(13.))
                            .text_color(if shuffle || best {
                                theme::ACCENT
                            } else {
                                theme::TEXT
                            }),
                    )
                    .tooltip(move |_, cx| cx.new(|_| TextTip(tip.into())).into())
                    .on_click(cx.listener(|s, _, window, cx| {
                        s.open_order_menu(window, cx);
                    })),
            )
            .children(self.order_status_text().map(|status| {
                div()
                    .text_size(px(9.))
                    .text_color(theme::MUTED)
                    .child(status)
            }))
            .child(
                button("automix-pause", paused, enabled)
                    .w(px(26.))
                    .child(
                        svg()
                            .path(if paused {
                                "icons/play.svg"
                            } else {
                                "icons/pause.svg"
                            })
                            .size(px(14.))
                            .text_color(if paused { theme::ACCENT } else { theme::TEXT }),
                    )
                    .tooltip(move |_, cx| {
                        cx.new(|_| {
                            TextTip(
                                if paused {
                                    "Resume AutoMix"
                                } else {
                                    "Pause AutoMix"
                                }
                                .into(),
                            )
                        })
                        .into()
                    })
                    .on_click(cx.listener(|s, _, _, cx| {
                        if s.snapshot.automix_on {
                            s.dispatch(if s.snapshot.automix_paused {
                                Command::ResumeAutomix
                            } else {
                                Command::PauseAutomix
                            });
                            cx.notify();
                        }
                    })),
            )
            .child(
                button("automix-skip", false, enabled)
                    .w(px(26.))
                    .child(
                        svg()
                            .path("icons/skip.svg")
                            .size(px(14.))
                            .text_color(theme::TEXT),
                    )
                    .tooltip(|_, cx| cx.new(|_| TextTip("Skip transition".into())).into())
                    .on_click(cx.listener(|s, _, _, cx| {
                        if s.snapshot.automix_on {
                            s.dispatch(Command::SkipAutomix);
                            cx.notify();
                        }
                    })),
            )
    }
}
