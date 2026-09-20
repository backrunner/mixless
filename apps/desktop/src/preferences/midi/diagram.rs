//! Code-drawn wireframe of the two decks, mixer, FX and browser.
use super::*;
use mixless_protocol::{DeckId, EqBand, StemKind};

#[derive(Clone, Copy)]
enum Shape {
    Button,
    Knob,
    Jog,
    Fader,
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
        let [x, y, w, h] = rect;
        let selected = self.binding.target == target;
        let assigned: Vec<_> = self
            .bindings
            .iter()
            .filter(|b| b.target == target)
            .collect();
        let color = if selected {
            theme::ACCENT
        } else if assigned.is_empty() {
            theme::MUTED
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
        let mut control = div()
            .id(SharedString::from(format!("map-diagram-{target:?}")))
            .absolute()
            .left(gpui::relative(x / 1000.))
            .top(px(y))
            .w(gpui::relative(w / 1000.))
            .h(px(h))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(2.))
            .text_size(px(8.))
            .text_color(color)
            .cursor_pointer()
            .rounded(px(3.))
            .when(selected, |el| el.bg(theme::with_alpha(theme::ACCENT, 0.10)))
            .hover(|el| el.bg(theme::PANEL_RAISED));
        control = match shape {
            Shape::Button => control
                .border_1()
                .border_color(color)
                .child(label.to_owned()),
            Shape::Knob | Shape::Jog => control
                .child(
                    div()
                        .relative()
                        .size(px(if matches!(shape, Shape::Jog) {
                            46.
                        } else {
                            15.
                        }))
                        .rounded_full()
                        .border_1()
                        .border_color(color)
                        .child(
                            div()
                                .absolute()
                                .top(px(2.))
                                .left(gpui::relative(0.5))
                                .w(px(1.))
                                .h(px(5.))
                                .bg(color),
                        ),
                )
                .child(label.to_owned()),
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
                                .h(px(4.))
                                .border_1()
                                .border_color(color),
                        ),
                )
                .child(label.to_owned()),
        };
        control
            .tooltip(move |_, cx| cx.new(|_| TextTip(tip.clone())).into())
            .on_click(cx.listener(move |s, _, _, cx| {
                s.select_midi_target(target.clone());
                cx.notify();
            }))
            .into_any_element()
    }

    pub(super) fn midi_diagram(&self, cx: &Context<Self>) -> gpui::AnyElement {
        use MidiTarget::*;
        let panel = |x: f32, w: f32, title: &'static str| {
            div()
                .absolute()
                .left(gpui::relative(x / 1000.))
                .top(px(30.))
                .w(gpui::relative(w / 1000.))
                .h(px(175.))
                .border_1()
                .border_color(theme::LINE)
                .rounded(px(4.))
                .pt_1()
                .px_2()
                .text_size(px(9.))
                .text_color(theme::MUTED)
                .child(title)
        };
        let mut diagram = div()
            .relative()
            .w_full()
            .h(px(281.))
            .flex_none()
            .child(panel(0., 300., "DECK A"))
            .child(panel(310., 380., "MIXER"))
            .child(panel(700., 300., "DECK B"));
        for (target, label, x, w) in [
            (Quantize, "Q", 0., 45.),
            (Automix, "AUTO", 55., 70.),
            (AutomixPause, "PAUSE", 135., 70.),
            (AutomixSkip, "SKIP", 215., 60.),
            (AutomixShuffle, "SHUFFLE", 285., 85.),
            (MasterGain, "MASTER GAIN", 690., 130.),
            (Master, "MASTER", 830., 80.),
            (CueGain, "CUE VOL", 920., 80.),
        ] {
            diagram =
                diagram.child(self.map_control(target, label, [x, 0., w, 22.], Shape::Button, cx));
        }
        for (deck, origin) in [(DeckId::A, 0.), (DeckId::B, 700.)] {
            for (target, label, r, shape) in [
                (Sync { deck }, "SYNC", [8., 51., 59., 18.], Shape::Button),
                (KeyLock { deck }, "LOCK", [8., 75., 59., 18.], Shape::Button),
                (Tempo { deck }, "TEMPO", [10., 99., 52., 65.], Shape::Fader),
                (Jog { deck }, "JOG", [80., 63., 105., 63.], Shape::Jog),
                (Play { deck }, "PLAY", [75., 130., 55., 20.], Shape::Button),
                (
                    TemporaryCue { deck },
                    "CUE",
                    [137., 130., 55., 20.],
                    Shape::Button,
                ),
                (
                    DeckGain { deck },
                    "GAIN",
                    [190., 57., 57., 34.],
                    Shape::Knob,
                ),
                (Balance { deck }, "BAL", [190., 96., 57., 32.], Shape::Knob),
                (Pitch { deck }, "KEY", [8., 169., 54., 30.], Shape::Knob),
                (Loop { deck }, "LOOP", [246., 56., 46., 18.], Shape::Button),
                (
                    LoopHalve { deck },
                    "÷2",
                    [246., 79., 46., 18.],
                    Shape::Button,
                ),
                (
                    LoopDouble { deck },
                    "×2",
                    [246., 102., 46., 18.],
                    Shape::Button,
                ),
                (
                    BeatBack { deck },
                    "←",
                    [246., 135., 21., 18.],
                    Shape::Button,
                ),
                (
                    BeatForward { deck },
                    "→",
                    [272., 135., 21., 18.],
                    Shape::Button,
                ),
            ] {
                let [x, y, w, h] = r;
                let x = if deck == DeckId::B { 300. - x - w } else { x };
                diagram = diagram.child(self.map_control(
                    target,
                    label,
                    [origin + x, y, w, h],
                    shape,
                    cx,
                ));
            }
            for index in 0..8 {
                diagram = diagram.child(self.map_control(
                    Cue { deck, index },
                    &(index + 1).to_string(),
                    [
                        origin
                            + if deck == DeckId::A { 76. } else { 109. }
                            + (index % 4) as f32 * 30.,
                        156. + (index / 4) as f32 * 23.,
                        25.,
                        18.,
                    ],
                    Shape::Button,
                    cx,
                ));
            }
            for (i, stem) in [StemKind::Vocals, StemKind::Drums, StemKind::Instruments]
                .into_iter()
                .enumerate()
            {
                diagram = diagram.child(self.map_control(
                    Stem { deck, stem },
                    ["V", "D", "I"][i],
                    [
                        origin + if deck == DeckId::A { 210. } else { 65. },
                        142. + i as f32 * 19.,
                        25.,
                        16.,
                    ],
                    Shape::Button,
                    cx,
                ));
            }
            for slot in 0..4 {
                let x = origin + if deck == DeckId::A { slot } else { 3 - slot } as f32 * 75.;
                diagram = diagram
                    .child(self.map_control(
                        FxOn { deck, slot },
                        &format!("FX {}", slot + 1),
                        [x, 214., 40., 21.],
                        Shape::Button,
                        cx,
                    ))
                    .child(self.map_control(
                        FxMix { deck, slot },
                        "",
                        [x + 42., 211., 28., 26.],
                        Shape::Knob,
                        cx,
                    ));
            }
        }
        for (deck, x) in [(DeckId::A, 322.), (DeckId::B, 508.)] {
            for (target, label, r, shape) in [
                (Gain { deck }, "TRIM", [0., 52., 53., 30.], Shape::Knob),
                (Filter { deck }, "FILTER", [58., 52., 58., 30.], Shape::Knob),
                (
                    Resonance { deck },
                    "RES",
                    [120., 52., 50., 30.],
                    Shape::Knob,
                ),
                (
                    Eq {
                        deck,
                        band: EqBand::High,
                    },
                    "HI",
                    [7., 86., 50., 30.],
                    Shape::Knob,
                ),
                (
                    Eq {
                        deck,
                        band: EqBand::Mid,
                    },
                    "MID",
                    [7., 119., 50., 30.],
                    Shape::Knob,
                ),
                (
                    Eq {
                        deck,
                        band: EqBand::Low,
                    },
                    "LOW",
                    [7., 152., 50., 30.],
                    Shape::Knob,
                ),
                (Fader { deck }, "LEVEL", [73., 88., 67., 70.], Shape::Fader),
                (Pfl { deck }, "PFL", [78., 165., 60., 18.], Shape::Button),
            ] {
                let [dx, y, w, h] = r;
                diagram =
                    diagram.child(self.map_control(target, label, [x + dx, y, w, h], shape, cx));
            }
        }
        diagram = diagram.child(self.map_control(
            Xfader,
            "← CROSSFADER →",
            [350., 214., 300., 21.],
            Shape::Button,
            cx,
        ));
        for (target, label, x, w) in [
            (BrowsePlaylists, "PLAYLIST ↕", 0., 180.),
            (BrowseTracks, "TRACK ↕", 190., 250.),
            (LoadSelected { deck: DeckId::A }, "LOAD A", 455., 130.),
            (LoadSelected { deck: DeckId::B }, "LOAD B", 600., 130.),
            (LoadFocused, "LOAD FOCUS", 745., 155.),
        ] {
            diagram = diagram.child(self.map_control(
                target,
                label,
                [x, 250., w, 22.],
                Shape::Button,
                cx,
            ));
        }
        diagram
            .child(
                div()
                    .absolute()
                    .right_0()
                    .bottom_0()
                    .text_size(px(8.))
                    .text_color(theme::LED_GREEN)
                    .child("● Mapped"),
            )
            .into_any_element()
    }
}
