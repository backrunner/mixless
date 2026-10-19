//! Scheduled source windows and live controls, summarized in the top bar.
use crate::{state::UiState, theme};
use gpui::{IntoElement, div, prelude::*, px};
use mixless_protocol::{DeckId, EngineSnapshot, MixPlan, StrategyId as S};

use super::TextTip;
mod loading;
mod tooltip;
use tooltip::AutomixTip;
#[cfg(debug_assertions)]
pub(crate) mod preview;

fn time(seconds: f32) -> String {
    let seconds = seconds.max(0.).round() as u32;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
fn label(deck: DeckId) -> &'static str {
    if deck == DeckId::A { "A" } else { "B" }
}
fn technique(strategy: S) -> &'static str {
    match strategy {
        S::DryCut => "Phrase cut",
        S::BassSwap => "Bass swap",
        S::PhraseBlend | S::BreakToIntro => "Phrase blend",
        S::EchoOut => "Echo out",
        S::FilterSweep | S::FallbackSwapFilter => "Filter blend",
        S::DropCut => "Drop cut",
        S::LoopConstruct => "Loop roll",
        S::ScratchCut => "Scratch cut",
        S::Spinback => "Spinback",
        S::TapeStop => "Tape stop",
        S::BuildRoll => "Build roll",
        S::StemSwap => "Stem swap",
        S::VocalRide => "Vocal ride",
        S::LoopOut => "Loop out",
        S::EnergyHold => "Energy hold",
    }
}

/// One-line transition summary plus the detail rows behind it.
#[derive(Clone, PartialEq)]
struct Transition {
    out: DeckId,
    incoming: DeckId,
    name: &'static str,
    status: String,
    paused: bool,
    phase: String,
    windows: [String; 2],
    parameters: [String; 3],
    stages: Vec<String>,
    active_stage: Option<usize>,
    echo: Option<String>,
}

impl Transition {
    fn new(out: DeckId, plan: &MixPlan, snapshot: &EngineSnapshot) -> Self {
        let incoming = if out == DeckId::A {
            DeckId::B
        } else {
            DeckId::A
        };
        let a = snapshot.deck(out);
        let b = snapshot.deck(incoming);
        let now = a.frame as f32 / a.src_sample_rate.max(1) as f32;
        let until = (plan.t_in_a - now) / a.rate.max(0.01);
        let stage = plan.stage_at_elapsed(snapshot.automix_progress * plan.duration_sec());
        let active_stage = (until <= 0.)
            .then(|| {
                stage.and_then(|active| plan.stages.iter().position(|s| std::ptr::eq(s, active)))
            })
            .flatten();
        let mut phase = if snapshot.automix_paused {
            "Paused".into()
        } else if until > 0. {
            format!("Starts in {}", time(until))
        } else {
            format!("Transition {:.0}%", snapshot.automix_progress * 100.)
        };
        let name: &'static str = plan
            .summary
            .as_ref()
            .map(|s| technique(s.strategy))
            .unwrap_or("Transition");
        // A published technique describes the whole plan. Its incoming deck
        // may still be cued, or playing a silent lead-in with all stems shut.
        // Do not present either state as an exchange already in progress.
        let closed = |deck: &mixless_protocol::DeckSnapshot| {
            deck.fader <= 0. || (deck.stems_ready && deck.stem_gain.iter().all(|gain| *gain <= 0.))
        };
        let status = if snapshot.automix_paused {
            "Paused".into()
        } else if until > 0. {
            format!("In {}", time(until))
        } else if closed(b) {
            phase = format!("Waiting for {} entry", label(incoming));
            "Waiting".into()
        } else if closed(a) {
            phase = "Completing transition".into();
            "Finishing".into()
        } else {
            name.into()
        };
        let windows = [
            format!("{}–{}", time(plan.t_in_a), time(plan.t_out_a)),
            format!("{}–{}", time(plan.t_in_b), time(plan.t_end_b)),
        ];
        let parameters = [
            format!("{:.1}", b.sounding_bpm),
            format!("{:+.0}", b.pitch_semitones),
            format!("{:.0}%", b.fader * 100.),
        ];
        // Planner labels can contain inline separators. Present each stage as
        // its own wrapped row, with ordinary punctuation within the sentence.
        let stages = plan
            .stages
            .iter()
            .map(|s| s.label.replace(" · ", ", "))
            .collect();
        let echo = (a.send > 0.001 || b.send > 0.001).then(|| {
            format!(
                "ECHO  {} {:.0}%  /  {} {:.0}%",
                label(out),
                a.send * 100.,
                label(incoming),
                b.send * 100.
            )
        });
        Self {
            out,
            incoming,
            name,
            status,
            paused: snapshot.automix_paused,
            phase,
            windows,
            parameters,
            stages,
            active_stage,
            echo,
        }
    }
}

impl UiState {
    pub fn transition_window(&self, deck: DeckId) -> Option<(f32, f32)> {
        let (out, plan) = self.automix_plan.as_ref()?;
        let summary = plan.summary.as_ref()?;
        let (id, window) = if *out == deck {
            (summary.pair.0, (plan.t_in_a, plan.t_out_a))
        } else {
            (summary.pair.1, (plan.t_in_b, plan.t_end_b))
        };
        (self.automix_active && self.deck(deck).track_id == Some(id)).then_some(window)
    }

    fn transition(&self) -> Option<Transition> {
        let (out, plan) = self.automix_plan.as_ref()?;
        Some(Transition::new(*out, plan, &self.snapshot))
    }

    /// Compact transition status for the top bar. Hover shows the details as a
    /// tooltip.
    pub fn render_automix_chip(&self, compact: bool, cx: &gpui::Context<Self>) -> gpui::AnyElement {
        let Some(transition) = self.transition() else {
            let status = self.automix_status.clone();
            return div()
                .id("automix-status")
                .debug_selector(|| "automix-status".into())
                .flex()
                .items_center()
                .gap(px(6.))
                .min_w_0()
                .max_w(px(if compact { 190. } else { 280. }))
                .flex_shrink()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_size(px(10.))
                .text_color(theme::MUTED)
                .when(self.automix_loading, |row| row.child(loading::indicator()))
                .child(div().min_w_0().truncate().child(status.clone()))
                .when_some(self.automix_preparation_progress, |row, progress| {
                    row.child(
                        div()
                            .flex_none()
                            .text_color(theme::TEXT)
                            .child(format!("{:.0}%", (progress * 100.).floor())),
                    )
                })
                .tooltip(move |_, cx| cx.new(|_| TextTip(status.clone())).into())
                .into_any_element();
        };
        let live = if transition.paused {
            theme::WARN
        } else {
            theme::LED_GREEN
        };
        let segment = |color, text: &'static str| div().flex_none().text_color(color).child(text);
        let anchor = std::rc::Rc::new(std::cell::Cell::new(None::<gpui::Bounds<gpui::Pixels>>));
        let measured_anchor = anchor.clone();
        let state = cx.weak_entity();
        div()
            .id("automix-transition")
            .debug_selector(|| "automix-transition".into())
            .relative()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(if compact { 4. } else { 6. }))
            .h(px(20.))
            .max_w(px(200.))
            .overflow_hidden()
            .whitespace_nowrap()
            .px(px(if compact { 5. } else { 9. }))
            .rounded(px(10.))
            .border_1()
            .border_color(theme::with_alpha(live, 0.3))
            .bg(theme::with_alpha(live, 0.06))
            .text_size(px(if compact { 8. } else { 10. }))
            .hover(|s| {
                s.border_color(theme::with_alpha(live, 0.55))
                    .bg(theme::with_alpha(live, 0.12))
            })
            .child(div().flex_none().size(px(5.)).rounded_full().bg(live))
            .child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(if compact { 3. } else { 4. }))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(segment(
                        theme::deck_color(transition.out),
                        label(transition.out),
                    ))
                    .child(segment(theme::MUTED, "→"))
                    .child(segment(
                        theme::deck_color(transition.incoming),
                        label(transition.incoming),
                    ))
                    .when(!compact || transition.status != transition.name, |row| {
                        row.child(
                            div().ml(px(if compact { 1. } else { 3. })).child(
                                div()
                                    .text_color(theme::TEXT)
                                    .child(transition.status.clone()),
                            ),
                        )
                    }),
            )
            .child(
                gpui::canvas(
                    move |bounds, _, _| measured_anchor.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .hoverable_tooltip(move |window, cx| {
                let gap = anchor.get().map_or(12., |bounds| {
                    f32::from(bounds.bottom() - window.mouse_position().y).max(0.) + 6.
                });
                cx.new(|cx| AutomixTip::new(transition.clone(), gap, state.clone(), cx))
                    .into()
            })
            .into_any_element()
    }
}

#[cfg(test)]
mod tests;
