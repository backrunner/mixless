//! Clickable wireframe following the native deck, mixer and full-width FX layout.
use super::*;
use mixless_protocol::{DeckId, EqBand, StemKind};

#[derive(Clone, Copy)]
enum Shape {
    Button,
    Knob,
    Jog,
    Fader,
    Crossfader,
}

impl Preferences {
    fn map_control(
        &self,
        target: MidiTarget,
        label: &str,
        rect: [f32; 4],
        shape: Shape,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let h = rect[3];
        let selected = self.binding.target == target;
        let assigned: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.target == target)
            .collect();
        let color = if selected && self.quick_learning {
            theme::ACCENT
        } else if assigned.is_empty() {
            if selected {
                theme::ACCENT
            } else {
                theme::MUTED
            }
        } else {
            theme::LED_GREEN
        };
        let tip = format!(
            "{}\n{}",
            target.label(),
            if assigned.is_empty() {
                "Unmapped".into()
            } else {
                assigned
                    .iter()
                    .map(|b| {
                        format!(
                            "{} · {}",
                            b.device_id
                                .as_ref()
                                .map_or("Any input".into(), |id| self.port_name(id)),
                            self.binding_text(b)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        );
        let mut control = diagram_rect(rect)
            // Reset a visible tooltip when an automatic bind changes its text.
            .id(SharedString::from(format!("map-diagram-{target:?}-{tip}")))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .text_size(px(if matches!(shape, Shape::Knob) { 7. } else { 8. }))
            .line_height(px(10.))
            .whitespace_nowrap()
            .text_color(color)
            .cursor_pointer()
            .rounded(px(3.))
            .when(selected, |el| {
                el.bg(theme::with_alpha(
                    color,
                    if self.quick_learning { 0.18 } else { 0.08 },
                ))
            })
            .hover(|el| el.bg(theme::PANEL_RAISED));
        control = match shape {
            Shape::Button => control
                .border_1()
                .border_color(theme::with_alpha(color, if selected { 0.9 } else { 0.45 }))
                .child(label.to_owned()),
            Shape::Knob => control
                .child(
                    div()
                        .relative()
                        .flex_none()
                        .size(px(14.))
                        .rounded_full()
                        .border_1()
                        .border_color(color)
                        .child(
                            div()
                                .absolute()
                                .top(px(2.))
                                .left(gpui::relative(0.5))
                                .w(px(1.))
                                .h(px(4.))
                                .bg(color),
                        ),
                )
                .when(!label.is_empty(), |el| el.child(label.to_owned())),
            Shape::Jog => control.child(
                div()
                    .relative()
                    .flex_none()
                    .size(px(84.))
                    .rounded_full()
                    .border_1()
                    .border_color(color)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(74.))
                            .rounded_full()
                            .border_1()
                            .border_color(theme::with_alpha(color, 0.35))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                div()
                                    .size(px(52.))
                                    .rounded_full()
                                    .border_1()
                                    .border_color(theme::with_alpha(color, 0.55))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(label.to_owned()),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(1.))
                            .left(gpui::relative(0.5))
                            .w(px(2.))
                            .h(px(6.))
                            .bg(color),
                    ),
            ),
            Shape::Fader => control
                .child(
                    div()
                        .relative()
                        .w(px(2.))
                        .h(px((h - 16.).max(16.)))
                        .bg(theme::LINE)
                        .child(
                            div()
                                .absolute()
                                .left(px(-4.))
                                .top(gpui::relative(0.45))
                                .w(px(10.))
                                .h(px(6.))
                                .bg(theme::PANEL_RAISED)
                                .border_1()
                                .border_color(color),
                        ),
                )
                .when(!label.is_empty(), |el| el.child(label.to_owned())),
            Shape::Crossfader => control.px_1().child(
                div().relative().w_full().h(px(2.)).bg(theme::LINE).child(
                    div()
                        .absolute()
                        .left(gpui::relative(0.5))
                        .top(px(-4.))
                        .w(px(5.))
                        .h(px(10.))
                        .bg(theme::PANEL_RAISED)
                        .border_1()
                        .border_color(color),
                ),
            ),
        };
        control
            .tooltip(move |_, cx| cx.new(|_| TextTip(tip.clone())).into())
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(move |s, _, window, cx| {
                    window.prevent_default();
                    s.quick_learn_target(target.clone());
                    cx.notify();
                }),
            )
            .into_any_element()
    }

    pub(super) fn midi_diagram(&self, cx: &Context<Self>) -> gpui::AnyElement {
        use MidiTarget::*;
        let mut diagram = div()
            .relative()
            .w_full()
            .h(px(342.))
            .flex_none()
            .child(diagram_panel([0., 36., 370., 204.]))
            .child(diagram_panel([380., 36., 240., 204.]))
            .child(diagram_panel([630., 36., 370., 204.]))
            .child(diagram_panel([0., 248., 1000., 54.]));
        for (target, label, x, w) in [
            (Quantize, "Q", 0., 40.),
            (Automix, "AUTO", 50., 65.),
            (AutomixPause, "PAUSE", 125., 65.),
            (AutomixSkip, "SKIP", 200., 55.),
            (AutomixShuffle, "SHUFFLE", 265., 75.),
        ] {
            diagram =
                diagram.child(self.map_control(target, label, [x, 4., w, 21.], Shape::Button, cx));
        }
        for (target, label, x) in [
            (MasterGain, "MASTER GAIN", 690.),
            (Master, "MASTER", 800.),
            (CueGain, "CUE VOL", 910.),
        ] {
            diagram =
                diagram.child(self.map_control(target, label, [x, 0., 90., 30.], Shape::Knob, cx));
        }
        for deck in [DeckId::A, DeckId::B] {
            diagram = self.diagram_deck(diagram, deck, cx);
            diagram = self.diagram_channel(diagram, deck, cx);
            diagram = self.diagram_fx(diagram, deck, cx);
        }
        diagram = diagram
            .child(diagram_label("A", [410., 220., 18., 16.], theme::DECK_A))
            .child(self.map_control(Xfader, "", [428., 220., 144., 16.], Shape::Crossfader, cx))
            .child(diagram_label("B", [572., 220., 18., 16.], theme::DECK_B))
            .child(diagram_label("FX", [480., 267., 40., 16.], theme::MUTED));
        for (target, label, x, w) in [
            (BrowsePlaylists, "PLAYLIST ↕", 0., 165.),
            (BrowseTracks, "TRACK ↕", 175., 205.),
            (LoadSelected { deck: DeckId::A }, "LOAD A", 390., 120.),
            (LoadSelected { deck: DeckId::B }, "LOAD B", 520., 120.),
            (LoadFocused, "LOAD FOCUS", 650., 160.),
        ] {
            diagram = diagram.child(self.map_control(
                target,
                label,
                [x, 312., w, 22.],
                Shape::Button,
                cx,
            ));
        }
        diagram
            .child(diagram_label(
                "● Mapped",
                [870., 315., 130., 16.],
                theme::LED_GREEN,
            ))
            .into_any_element()
    }

    fn diagram_deck(&self, mut diagram: gpui::Div, deck: DeckId, cx: &Context<Self>) -> gpui::Div {
        use MidiTarget::*;
        // Match deck.rs: tempo/stems outside, jog in the middle, gain/balance
        // inside. Only the body and performance groups mirror; cue numbers
        // and the track header still read left to right on both decks.
        let origin = if deck == DeckId::A { 0. } else { 630. };
        let rect = |[x, y, w, h]: [f32; 4]| {
            [
                origin + (if deck == DeckId::A { x } else { 390. - x - w }) * 370. / 390.,
                y,
                w * 370. / 390.,
                h,
            ]
        };
        diagram = diagram
            .child(diagram_label(
                if deck == DeckId::A {
                    "DECK A"
                } else {
                    "DECK B"
                },
                [origin + 10., 43., 70., 16.],
                theme::deck_color(deck),
            ))
            .child(diagram_rect([origin + 90., 49., 125., 2.]).bg(theme::LINE))
            .child(diagram_rect([origin + 90., 55., 75., 1.]).bg(theme::LINE_SOFT))
            .child(diagram_label(
                "BPM / KEY",
                [origin + 260., 43., 100., 16.],
                theme::MUTED,
            ));
        for (target, label, r, shape) in [
            (Tempo { deck }, "TEMPO", [8., 94., 34., 90.], Shape::Fader),
            (Sync { deck }, "SYNC", [49., 71., 65., 17.], Shape::Button),
            (
                KeyLock { deck },
                "KEY LOCK",
                [49., 92., 65., 17.],
                Shape::Button,
            ),
            (Pitch { deck }, "KEY", [118., 71., 34., 30.], Shape::Knob),
            (Jog { deck }, "JOG", [163., 77., 164., 107.], Shape::Jog),
            (
                DeckGain { deck },
                "GAIN",
                [335., 73., 47., 31.],
                Shape::Knob,
            ),
            (
                Balance { deck },
                "BALANCE",
                [329., 151., 59., 31.],
                Shape::Knob,
            ),
            (Play { deck }, "▶", [10., 213., 48., 20.], Shape::Button),
            (
                TemporaryCue { deck },
                "CUE",
                [64., 213., 48., 20.],
                Shape::Button,
            ),
            (
                LoopDouble { deck },
                "▲",
                [325., 190., 55., 11.],
                Shape::Button,
            ),
            (Loop { deck }, "LOOP", [325., 204., 55., 16.], Shape::Button),
            (
                LoopHalve { deck },
                "▼",
                [325., 222., 55., 11.],
                Shape::Button,
            ),
        ] {
            diagram = diagram.child(self.map_control(target, label, rect(r), shape, cx));
        }
        // SHIFT is a native UI modifier, not an independently mappable target.
        diagram = diagram.child(
            diagram_label("SHIFT", rect([8., 71., 34., 17.]), theme::MUTED)
                .border_1()
                .border_color(theme::LINE)
                .rounded(px(2.)),
        );
        // Keep the wireframe in the same 2×2 order as the native deck.
        for (stem, label, r) in [
            (StemKind::Vocals, "VOCAL", [49., 110., 32., 26.]),
            (StemKind::Drums, "DRUMS", [84., 110., 32., 26.]),
            (StemKind::Bass, "BASS", [49., 140., 32., 26.]),
            (StemKind::Instruments, "OTHER", [84., 140., 32., 26.]),
        ] {
            diagram = diagram.child(self.map_control(
                Stem { deck, stem },
                label,
                rect(r),
                Shape::Knob,
                cx,
            ));
        }
        let pads_x = rect([120., 0., 194., 0.])[0];
        for index in 0..8 {
            diagram = diagram.child(self.map_control(
                Cue { deck, index },
                &(index + 1).to_string(),
                [
                    pads_x + (index % 4) as f32 * 49. * 370. / 390.,
                    190. + (index / 4) as f32 * 23.,
                    47. * 370. / 390.,
                    20.,
                ],
                Shape::Button,
                cx,
            ));
        }
        diagram
    }

    fn diagram_channel(
        &self,
        mut diagram: gpui::Div,
        deck: DeckId,
        cx: &Context<Self>,
    ) -> gpui::Div {
        use MidiTarget::*;
        let origin = if deck == DeckId::A { 384. } else { 504. };
        diagram = diagram.child(diagram_label(
            if deck == DeckId::A { "A" } else { "B" },
            [origin, 41., 112., 13.],
            theme::deck_color(deck),
        ));
        // TRIM/FILTER/RES retain their order; EQ and fader columns mirror.
        for (i, (target, label)) in [
            (Gain { deck }, "TRIM"),
            (Filter { deck }, "FILTER"),
            (Resonance { deck }, "RES"),
        ]
        .into_iter()
        .enumerate()
        {
            diagram = diagram.child(self.map_control(
                target,
                label,
                [origin + i as f32 * 38., 58., 34., 29.],
                Shape::Knob,
                cx,
            ));
        }
        let (eq_x, kill_x, fader_x, meter_x) = if deck == DeckId::A {
            (405., 386., 456., 445.)
        } else {
            (559., 600., 509., 549.)
        };
        for (i, (band, label)) in [
            (EqBand::High, "HI"),
            (EqBand::Mid, "MID"),
            (EqBand::Low, "LOW"),
        ]
        .into_iter()
        .enumerate()
        {
            let y = 94. + i as f32 * 36.;
            diagram = diagram
                .child(self.map_control(
                    Eq { deck, band },
                    label,
                    [eq_x, y, 36., 30.],
                    Shape::Knob,
                    cx,
                ))
                .child(self.map_control(
                    EqKill { deck, band },
                    "",
                    [kill_x, y + 7., 14., 10.],
                    Shape::Button,
                    cx,
                ));
        }
        for row in 0..18 {
            for side in 0..2 {
                diagram = diagram.child(
                    diagram_rect([meter_x + side as f32 * 4., 94. + row as f32 * 5.5, 2., 3.])
                        .bg(theme::LINE),
                );
            }
        }
        diagram
            .child(self.map_control(
                Fader { deck },
                "",
                [fader_x, 91., 35., 105.],
                Shape::Fader,
                cx,
            ))
            .child(self.map_control(
                Pfl { deck },
                "PFL",
                [fader_x - 2., 202., 39., 15.],
                Shape::Button,
                cx,
            ))
    }

    fn diagram_fx(&self, mut diagram: gpui::Div, deck: DeckId, cx: &Context<Self>) -> gpui::Div {
        use MidiTarget::*;
        let rect =
            |[x, y, w, h]: [f32; 4]| [if deck == DeckId::A { x } else { 1000. - x - w }, y, w, h];
        diagram = diagram.child(diagram_label(
            if deck == DeckId::A { "A" } else { "B" },
            rect([6., 267., 20., 16.]),
            theme::deck_color(deck),
        ));
        for slot in 0..4 {
            let x = 32. + slot as f32 * 112.;
            diagram =
                diagram.child(diagram_panel(rect([x, 253., 106., 44.])).bg(theme::PANEL_INSET));
            for (target, label, r, shape) in [
                (
                    FxPrevious { deck, slot },
                    if deck == DeckId::A { "◀" } else { "▶" },
                    [x + 4., 257., 14., 16.],
                    Shape::Button,
                ),
                (
                    FxOn { deck, slot },
                    "",
                    [x + 22., 257., 62., 16.],
                    Shape::Button,
                ),
                (
                    FxNext { deck, slot },
                    if deck == DeckId::A { "▶" } else { "◀" },
                    [x + 88., 257., 14., 16.],
                    Shape::Button,
                ),
                (
                    FxMix { deck, slot },
                    "",
                    [x + 26., 276., 22., 18.],
                    Shape::Knob,
                ),
            ] {
                let label = if matches!(target, FxOn { .. }) {
                    format!("FX {}", slot + 1)
                } else {
                    label.into()
                };
                diagram = diagram.child(self.map_control(target, &label, rect(r), shape, cx));
            }
            // HOLD has no MIDI target; retain its outline as a visual landmark.
            diagram = diagram
                .child(diagram_label(
                    "MIX",
                    rect([x + 4., 278., 22., 14.]),
                    theme::MUTED,
                ))
                .child(diagram_label(
                    "···",
                    rect([x + 50., 277., 15., 14.]),
                    theme::MUTED,
                ))
                .child(
                    diagram_label("HOLD", rect([x + 68., 276., 34., 18.]), theme::MUTED)
                        .border_1()
                        .border_color(theme::LINE)
                        .rounded(px(2.)),
                );
        }
        diagram
    }
}

// Horizontal coordinates share a 1000-unit viewbox; vertical dimensions stay
// legible at the Preferences window's 720px minimum width.
fn diagram_rect([x, y, w, h]: [f32; 4]) -> gpui::Div {
    div()
        .absolute()
        .left(gpui::relative(x / 1000.))
        .top(px(y))
        .w(gpui::relative(w / 1000.))
        .h(px(h))
}

fn diagram_panel(rect: [f32; 4]) -> gpui::Div {
    diagram_rect(rect)
        .border_1()
        .border_color(theme::LINE)
        .rounded(px(4.))
}

fn diagram_label(label: &str, rect: [f32; 4], color: gpui::Rgba) -> gpui::Div {
    diagram_rect(rect)
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(7.))
        .line_height(px(10.))
        .whitespace_nowrap()
        .text_color(color)
        .child(label.to_owned())
}
