//! Scheduled source windows and live controls, summarized in the top bar.
use crate::{state::UiState, theme};
use gpui::{IntoElement, Window, div, prelude::*, px};
use mixless_protocol::{DeckId, StrategyId as S};

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
        S::EnergyHold => "Energy hold",
    }
}

/// One-line transition summary plus the detail rows behind it.
#[derive(Clone)]
struct Transition {
    summary: String,
    lines: Vec<String>,
    echo: Option<String>,
}

struct AutomixTip(Transition);
impl gpui::Render for AutomixTip {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        let mut tip = div()
            .px_3()
            .py_2()
            .rounded(px(4.))
            .bg(theme::PANEL_RAISED)
            .flex()
            .flex_col()
            .gap_1()
            .text_size(px(10.))
            .text_color(theme::MUTED)
            .children(self.0.lines.iter().map(|line| div().child(line.clone())));
        if let Some(echo) = &self.0.echo {
            tip = tip.child(div().text_color(theme::ACCENT).child(echo.clone()));
        }
        tip
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
        let incoming = if *out == DeckId::A {
            DeckId::B
        } else {
            DeckId::A
        };
        let a = self.deck(*out);
        let b = self.deck(incoming);
        let now = a.frame as f32 / a.src_sample_rate.max(1) as f32;
        let until = (plan.t_in_a - now) / a.rate.max(0.01);
        let phase = if self.snapshot.automix_paused {
            "Paused".into()
        } else if until > 0. {
            format!("Ready · starts in {}", time(until))
        } else {
            let stage = plan
                .stage_at_elapsed(self.snapshot.automix_progress * plan.duration_sec())
                .map_or("Finish transition", |s| s.label.as_str());
            format!("{stage} · {:.0}%", self.snapshot.automix_progress * 100.)
        };
        let name = plan
            .summary
            .as_ref()
            .map(|s| technique(s.strategy))
            .unwrap_or("Transition");
        let summary = format!(
            "{} → {}  ·  {name}  ·  {phase}",
            label(*out),
            label(incoming)
        );
        let lines = vec![
            format!(
                "{} OUT {}–{}   /   {} IN {}–{}",
                label(*out),
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
        let echo = (a.send > 0.001 || b.send > 0.001).then(|| {
            format!(
                "ECHO  {} {:.0}%  /  {} {:.0}%",
                label(*out),
                a.send * 100.,
                label(incoming),
                b.send * 100.
            )
        });
        Some(Transition {
            summary,
            lines,
            echo,
        })
    }

    /// Compact transition status for the top bar. Hover shows the details as a
    /// tooltip; click pins them in a popover.
    pub fn render_automix_chip(&self, cx: &mut gpui::Context<Self>) -> gpui::AnyElement {
        let Some(transition) = self.transition() else {
            return div()
                .flex_none()
                .max_w(px(220.))
                .truncate()
                .text_size(px(10.))
                .text_color(theme::MUTED)
                .child(self.automix_status.clone())
                .into_any_element();
        };
        div()
            .id("automix-transition")
            .flex_none()
            .max_w(px(320.))
            .truncate()
            .px_2()
            .py(px(2.))
            .rounded(px(4.))
            .text_size(px(10.))
            .text_color(theme::LED_GREEN)
            .cursor_pointer()
            .hover(|s| s.bg(theme::PANEL_RAISED))
            .child(transition.summary.clone())
            .tooltip(move |_, cx| cx.new(|_| AutomixTip(transition.clone())).into())
            .on_click(cx.listener(|s, _, _, cx| {
                s.automix_detail = !s.automix_detail;
                cx.notify();
            }))
            .into_any_element()
    }

    /// Pinned transition details under the top bar; dismissed by backdrop or
    /// Escape, hidden whenever there is no active plan.
    pub fn render_automix_detail(&self, cx: &mut gpui::Context<Self>) -> Option<gpui::AnyElement> {
        if !self.automix_detail {
            return None;
        }
        let transition = self.transition()?;
        let mut panel = div()
            .id("automix-detail-panel")
            .flex()
            .flex_col()
            .gap_1()
            .p_3()
            .rounded(px(theme::POPUP_RADIUS))
            .bg(theme::PANEL_RAISED)
            .border_1()
            .border_color(theme::LINE)
            .shadow_lg()
            .text_size(px(10.))
            .text_color(theme::MUTED)
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(div().text_color(theme::LED_GREEN).child(transition.summary))
            .children(transition.lines.into_iter().map(|line| div().child(line)));
        if let Some(echo) = transition.echo {
            panel = panel.child(div().text_color(theme::ACCENT).child(echo));
        }
        Some(
            div()
                .absolute()
                .inset_0()
                .occlude()
                .flex()
                .items_start()
                .justify_center()
                .pt(px(52.))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|s, _, _, cx| {
                        s.automix_detail = false;
                        cx.notify();
                    }),
                )
                .child(panel)
                .into_any_element(),
        )
    }
}
