//! Scheduled source windows and live controls, summarized in the top bar.
use crate::{state::UiState, theme};
use gpui::{IntoElement, Window, div, prelude::*, px};
use mixless_protocol::{DeckId, EngineSnapshot, MixPlan, StrategyId as S};

use super::{TextTip, ellipsized_text};

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
        S::LoopOut => "Loop out",
        S::EnergyHold => "Energy hold",
    }
}

/// One-line transition summary plus the detail rows behind it.
#[derive(Clone)]
struct Transition {
    out: DeckId,
    incoming: DeckId,
    name: &'static str,
    paused: bool,
    summary: String,
    lines: Vec<String>,
    echo: Option<String>,
}

struct AutomixTip(Transition);
impl gpui::Render for AutomixTip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut tip = div()
            .max_w(px(400.))
            .px_3()
            .py_2()
            .rounded(px(4.))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(10.))
            .text_color(theme::MUTED)
            .whitespace_normal()
            .child(
                div()
                    .text_color(theme::TEXT)
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child(self.0.summary.clone()),
            )
            .children(self.0.lines.iter().map(|line| div().child(line.clone())));
        if let Some(echo) = &self.0.echo {
            tip = tip.child(div().text_color(theme::ACCENT).child(echo.clone()));
        }
        tip
    }
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
        let phase = if snapshot.automix_paused {
            "Paused".into()
        } else if until > 0. {
            format!("Starts in {}", time(until))
        } else {
            let stage = plan
                .stage_at_elapsed(snapshot.automix_progress * plan.duration_sec())
                .map_or("Finish transition", |s| s.label.as_str());
            format!("{stage} · {:.0}%", snapshot.automix_progress * 100.)
        };
        let name: &'static str = plan
            .summary
            .as_ref()
            .map(|s| technique(s.strategy))
            .unwrap_or("Transition");
        let summary = format!("{} → {}  ·  {name}", label(out), label(incoming));
        let mut lines = vec![
            phase,
            format!(
                "{} OUT {}–{}   /   {} IN {}–{}",
                label(out),
                time(plan.t_in_a),
                time(plan.t_out_a),
                label(incoming),
                time(plan.t_in_b),
                time(plan.t_end_b)
            ),
            format!(
                "{}  {:.1} BPM  ·  KEY {:+.0}  ·  LEVEL {:.0}%",
                label(incoming),
                b.sounding_bpm,
                b.pitch_semitones,
                b.fader * 100.
            ),
        ];
        if !plan.stages.is_empty() {
            lines.push(format!(
                "Stages: {}",
                plan.stages
                    .iter()
                    .map(|stage| stage.label.as_str())
                    .collect::<Vec<_>>()
                    .join(" → ")
            ));
        }
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
            paused: snapshot.automix_paused,
            summary,
            lines,
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
    pub fn render_automix_chip(&self) -> gpui::AnyElement {
        let Some(transition) = self.transition() else {
            let status = self.automix_status.clone();
            return div()
                .id("automix-status")
                .min_w_0()
                .w(px(220.))
                .flex_shrink()
                .overflow_hidden()
                .text_size(px(10.))
                .text_color(theme::MUTED)
                .child(ellipsized_text(status.clone()).w_full().h(px(14.)))
                .tooltip(move |_, cx| cx.new(|_| TextTip(status.clone())).into())
                .into_any_element();
        };
        let live = if transition.paused {
            theme::WARN
        } else {
            theme::LED_GREEN
        };
        let segment = |color, text: &'static str| div().flex_none().text_color(color).child(text);
        div()
            .id("automix-transition")
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(px(20.))
            .max_w(px(200.))
            .overflow_hidden()
            .whitespace_nowrap()
            .px(px(9.))
            .rounded(px(10.))
            .border_1()
            .border_color(theme::with_alpha(live, 0.3))
            .bg(theme::with_alpha(live, 0.06))
            .text_size(px(10.))
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
                    .gap(px(4.))
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
                    .child(segment(theme::MUTED, "·"))
                    .child(segment(theme::TEXT, transition.name)),
            )
            .tooltip(move |_, cx| cx.new(|_| AutomixTip(transition.clone())).into())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::{MixPlanSummary, MixStage, Polyline, TrackId};

    #[test]
    fn summary_stays_compact_while_tooltip_keeps_all_stages_and_status() {
        let plan = MixPlan {
            summary: Some(MixPlanSummary {
                pair: (TrackId(1), TrackId(2)),
                strategy: S::PhraseBlend,
                score: 0.8,
                used_fallback: false,
                length_bars: 8,
            }),
            t_in_a: 10.,
            t_out_a: 18.,
            t_in_b: 2.,
            t_end_b: 10.,
            clock: Polyline {
                nodes: vec![(0., 0.), (8., 8.)],
            },
            stages: vec![
                MixStage {
                    start_bar: 0.,
                    end_bar: 4.,
                    label: "Introduce filtered rhythm".into(),
                },
                MixStage {
                    start_bar: 4.,
                    end_bar: 8.,
                    label: "Bass exchange · echo tail".into(),
                },
            ],
            ..Default::default()
        };
        for out in [DeckId::A, DeckId::B] {
            let mut snapshot = EngineSnapshot::default();
            let ready = Transition::new(out, &plan, &snapshot);
            assert_eq!(
                ready.summary,
                if out == DeckId::A {
                    "A → B  ·  Phrase blend"
                } else {
                    "B → A  ·  Phrase blend"
                }
            );
            assert_eq!(ready.lines[0], "Starts in 0:10");
            assert!(ready.lines.iter().any(
                |line| line == "Stages: Introduce filtered rhythm → Bass exchange · echo tail"
            ));
            snapshot.decks[out.index()].frame = 15 * 44_100;
            snapshot.decks[out.index()].send = 0.5;
            snapshot.automix_progress = 0.625;
            let active = Transition::new(out, &plan, &snapshot);
            assert_eq!(active.summary, ready.summary);
            assert_eq!(active.lines[0], "Bass exchange · echo tail · 62%");
            assert!(active.echo.is_some());
            snapshot.automix_paused = true;
            let paused = Transition::new(out, &plan, &snapshot);
            assert_eq!(paused.summary, ready.summary);
            assert_eq!(paused.lines[0], "Paused");
        }
    }
}
