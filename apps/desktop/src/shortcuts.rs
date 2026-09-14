//! Keyboard performance controls. Resolve input separately from engine dispatch
//! so modifier, modal and key-repeat behavior can be checked without audio hardware.

use gpui::{KeyDownEvent, Keystroke};
use mixless_protocol::{Command, DeckId};

use crate::state::UiState;

#[derive(Debug, PartialEq)]
enum Shortcut {
    Help,
    Close,
    SwitchDeck,
    Play(DeckId),
    TemporaryCue(DeckId, bool),
    Cue(DeckId, usize, bool),
    QuickCue,
    Sync,
    Loop,
    LoopSize(bool),
    BeatJump(i16),
    CenterCrossfader,
}

pub const HELP: &[(&str, &str)] = &[
    ("Tab / Shift+Tab", "Switch selected deck A / B"),
    ("Space", "Play / pause selected deck"),
    ("Z / X", "Play / pause deck A / B"),
    (
        "C / Shift+C",
        "Set / return to temporary cue; clear with Shift",
    ),
    ("G", "Quick cue: set the next empty pad"),
    ("1–8", "Jump to selected deck cue; set if empty"),
    ("Q W E R / U I O P", "Deck A / B cues 1–4; set if empty"),
    ("Shift + cue key", "Choose cue use: AUTO / IN / OUT"),
    (
        "Hold CUE / pad",
        "Preview on monitor; release to return to cue",
    ),
    ("Hold Play", "Vinyl brake stop"),
    ("Right-click pad", "Delete saved cue"),
    ("S", "Sync selected deck"),
    ("L", "Toggle loop on selected deck"),
    ("[ / ]", "Halve / double loop size (1/16–64 beats)"),
    ("← / →", "Jump backward / forward 1 bar"),
    ("Shift + ← / →", "Jump backward / forward 4 bars"),
    ("F", "Center crossfader"),
    ("? / F1", "Show / hide keyboard shortcuts"),
    ("Esc", "Close this panel"),
];

fn resolve(ev: &KeyDownEvent, deck: DeckId, modal: bool, picker: bool) -> Option<Shortcut> {
    let Keystroke { key, modifiers, .. } = &ev.keystroke;
    if ev.is_held
        || picker
        || modifiers.control
        || modifiers.platform
        || modifiers.alt
        || modifiers.function
    {
        return None;
    }
    if key == "escape" {
        return Some(Shortcut::Close);
    }
    if modal {
        return None;
    }
    // GPUI on macOS represents Shift+1 as "!" with shift cleared (and
    // similarly for punctuation), while other platforms can retain shift.
    let shifted_digit = ["!", "@", "#", "$", "%", "^", "&", "*"]
        .iter()
        .position(|symbol| *symbol == key);
    if let Some(index) = shifted_digit {
        return Some(Shortcut::Cue(deck, index, true));
    }
    let key = key.to_ascii_lowercase();
    let cue = match key.as_str() {
        "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" => {
            Some((deck, key.as_bytes()[0] as usize - b'1' as usize))
        }

        _ => ["q", "w", "e", "r"]
            .iter()
            .position(|k| *k == key)
            .map(|i| (DeckId::A, i))
            .or_else(|| {
                ["u", "i", "o", "p"]
                    .iter()
                    .position(|k| *k == key)
                    .map(|i| (DeckId::B, i))
            }),
    };
    if let Some((deck, index)) = cue {
        return Some(Shortcut::Cue(deck, index, modifiers.shift));
    }
    match key.as_str() {
        "?" | "f1" => Some(Shortcut::Help),
        "/" if modifiers.shift => Some(Shortcut::Help),
        "tab" => Some(Shortcut::SwitchDeck),
        "c" => Some(Shortcut::TemporaryCue(deck, modifiers.shift)),
        "left" => Some(Shortcut::BeatJump(if modifiers.shift { -4 } else { -1 })),
        "right" => Some(Shortcut::BeatJump(if modifiers.shift { 4 } else { 1 })),
        _ if modifiers.shift => None,
        "space" => Some(Shortcut::Play(deck)),
        "z" => Some(Shortcut::Play(DeckId::A)),
        "x" => Some(Shortcut::Play(DeckId::B)),
        "g" => Some(Shortcut::QuickCue),
        "s" => Some(Shortcut::Sync),
        "l" => Some(Shortcut::Loop),
        "[" => Some(Shortcut::LoopSize(false)),
        "]" => Some(Shortcut::LoopSize(true)),
        "f" => Some(Shortcut::CenterCrossfader),
        _ => None,
    }
}

impl UiState {
    pub fn handle_shortcut(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        let action = resolve(
            ev,
            self.focus,
            self.show_import_modal || self.fx_editor.is_some(),
            self.picker_open,
        );
        let Some(action) = action else { return };
        if self.show_shortcuts && !matches!(action, Shortcut::Help | Shortcut::Close) {
            cx.stop_propagation();
            return;
        }
        match action {
            Shortcut::Help => self.show_shortcuts = !self.show_shortcuts,
            Shortcut::Close => {
                self.cue_role_editor = [None; 2];
                self.cue_shift = [false; 2];
                self.close_fx_editor();
                self.show_shortcuts = false;
                self.show_import_modal = false;
                self.playlist_menu = None;
                self.confirm_remove_playlist = None;
                self.keyboard_focus.focus(window);
            }
            Shortcut::SwitchDeck => {
                self.focus = if self.focus == DeckId::A {
                    DeckId::B
                } else {
                    DeckId::A
                }
            }
            Shortcut::Play(deck) => self.play_pause(deck),
            Shortcut::TemporaryCue(deck, clear) => self.temporary_cue(deck, clear),
            Shortcut::Cue(deck, index, replace) => self.trigger_cue(deck, index, replace),
            Shortcut::QuickCue => {
                // Take a fresh engine snapshot: several key presses can arrive
                // before the next UI frame has mirrored newly written cues.
                let snapshot = self.core.engine.snapshot();
                if snapshot.decks[self.focus.index()].track_id.is_some() {
                    if let Some(index) = snapshot.decks[self.focus.index()]
                        .cues
                        .iter()
                        .position(Option::is_none)
                    {
                        self.set_cue_now(self.focus, index);
                    } else {
                        self.error = "All 8 cues are set. Right-click a pad to clear it.".into();
                    }
                }
            }
            Shortcut::Sync => self.sync(self.focus),
            Shortcut::Loop => {
                let snapshot = self.core.engine.snapshot();
                let deck = &snapshot.decks[self.focus.index()];
                self.set_loop(self.focus, deck.loop_beats, !deck.loop_on);
            }
            Shortcut::LoopSize(double) => {
                let snapshot = self.core.engine.snapshot();
                let deck = &snapshot.decks[self.focus.index()];
                let bars = if double {
                    deck.loop_beats * 2.
                } else {
                    deck.loop_beats / 2.
                }
                .clamp(0.0625, 64.);
                self.set_loop(self.focus, bars, deck.loop_on);
            }
            Shortcut::BeatJump(bars) => self.dispatch(Command::BeatJump {
                deck: self.focus,
                bars,
            }),
            Shortcut::CenterCrossfader => self.dispatch(Command::SetCrossfader { value: 0.0 }),
        }
        cx.stop_propagation();
        window.prevent_default();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(key: &str) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke::parse(key).unwrap(),
            is_held: false,
        }
    }

    #[test]
    fn transport_targets_selected_or_explicit_deck() {
        assert_eq!(
            resolve(&event("space"), DeckId::B, false, false),
            Some(Shortcut::Play(DeckId::B))
        );
        assert_eq!(
            resolve(&event("z"), DeckId::B, false, false),
            Some(Shortcut::Play(DeckId::A))
        );
        assert_eq!(
            resolve(&event("x"), DeckId::A, false, false),
            Some(Shortcut::Play(DeckId::B))
        );
    }

    #[test]
    fn cues_cover_all_slots_and_preserve_deck_bindings() {
        for index in 0..8 {
            for deck in [DeckId::A, DeckId::B] {
                assert_eq!(
                    resolve(&event(&(index + 1).to_string()), deck, false, false),
                    Some(Shortcut::Cue(deck, index, false))
                );
                assert_eq!(
                    resolve(&event(&format!("shift-{}", index + 1)), deck, false, false),
                    Some(Shortcut::Cue(deck, index, true))
                );
            }
        }
        for (deck, keys) in [
            (DeckId::A, ["q", "w", "e", "r"]),
            (DeckId::B, ["u", "i", "o", "p"]),
        ] {
            for (index, key) in keys.iter().enumerate() {
                assert_eq!(
                    resolve(&event(key), DeckId::B, false, false),
                    Some(Shortcut::Cue(deck, index, false))
                );
                assert_eq!(
                    resolve(&event(&format!("shift-{key}")), DeckId::A, false, false),
                    Some(Shortcut::Cue(deck, index, true))
                );
            }
        }
        for (index, symbol) in ["!", "@", "#", "$", "%", "^", "&", "*"].iter().enumerate() {
            assert_eq!(
                resolve(&event(symbol), DeckId::B, false, false),
                Some(Shortcut::Cue(DeckId::B, index, true))
            );
        }
    }

    #[test]
    fn typing_modifiers_modals_and_repeat_do_not_trigger_performance() {
        for key in ["space", "g", "c", "1", "shift-8", "q", "l", "tab", "right"] {
            let ev = event(key);
            assert_eq!(resolve(&ev, DeckId::A, true, false), None);
            assert_eq!(resolve(&ev, DeckId::A, false, true), None);
            assert_eq!(
                resolve(
                    &KeyDownEvent {
                        is_held: true,
                        ..ev
                    },
                    DeckId::A,
                    false,
                    false
                ),
                None
            );
        }
        for key in ["cmd-q", "ctrl-space", "alt-1", "cmd-shift-1", "shift-space"] {
            assert_eq!(resolve(&event(key), DeckId::A, false, false), None);
        }
        assert_eq!(
            resolve(&event("escape"), DeckId::A, true, false),
            Some(Shortcut::Close)
        );
        assert_eq!(
            resolve(&event("shift-left"), DeckId::A, false, false),
            Some(Shortcut::BeatJump(-4))
        );
        assert_eq!(
            resolve(&event("?"), DeckId::A, false, false),
            Some(Shortcut::Help)
        );
    }
}
