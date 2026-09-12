//! Scheduled source windows and live controls, shown alongside the decks.
use crate::{state::UiState, theme};
use gpui::{IntoElement, prelude::*, px};
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
    pub fn render_automix_bar(&self) -> gpui::AnyElement {
        let mut row = gpui::div()
            .flex()
            .flex_none()
            .items_center()
            .justify_between()
            .gap_3()
            .h(px(28.))
            .px_3()
            .rounded(px(4.))
            .bg(theme::PANEL_INSET)
            .text_size(px(10.))
            .text_color(theme::MUTED)
            .overflow_hidden();
        let Some((out, plan)) = &self.automix_plan else {
            return row.child(self.automix_status.clone()).into_any_element();
        };
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
        row = row
            .child(gpui::div().text_color(theme::LED_GREEN).child(format!(
                "{} → {}  ·  {name}  ·  {phase}",
                label(*out),
                label(incoming)
            )))
            .child(format!(
                "{} OUT {}–{}   /   {} IN {}–{}",
                label(*out),
                time(plan.t_in_a),
                time(plan.t_out_a),
                label(incoming),
                time(plan.t_in_b),
                time(plan.t_end_b)
            ))
            .child(format!(
                "{}  {:.1} BPM  ·  KEY {:+.0}  ·  LEVEL {:.0}%",
                label(incoming),
                b.sounding_bpm,
                b.pitch_semitones,
                b.fader * 100.
            ));
        if a.send > 0.001 || b.send > 0.001 {
            row = row.child(gpui::div().text_color(theme::ACCENT).child(format!(
                "ECHO  {} {:.0}%  /  {} {:.0}%",
                label(*out),
                a.send * 100.,
                label(incoming),
                b.send * 100.
            )));
        }
        row.into_any_element()
    }
}
