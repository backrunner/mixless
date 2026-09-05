//! Mixless visual language: Pioneer DJ inspired matte black + amber.
//! No blue anywhere in the accent system.

use gpui::{Hsla, Rgba};

const fn c(hex: u32) -> Rgba {
    Rgba {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

pub const BG: Rgba = c(0x0a0a0b);
pub const PANEL: Rgba = c(0x141416);
pub const PANEL_RAISED: Rgba = c(0x1b1b1e);
pub const PANEL_INSET: Rgba = c(0x070708);
pub const LINE: Rgba = c(0x232327);
pub const LINE_SOFT: Rgba = c(0x1c1c1f);
pub const TEXT: Rgba = c(0xf0ede6);
pub const MUTED: Rgba = c(0x8b877e);

pub const ACCENT: Rgba = c(0xffb224);
pub const DECK_A: Rgba = c(0xffb224);
pub const DECK_B: Rgba = c(0xff5238);
pub const DANGER: Rgba = c(0xff3b30);
pub const WARN: Rgba = c(0xffb224);
pub const LED_GREEN: Rgba = c(0x34d399);
pub const LED_RED: Rgba = c(0xff4539);

pub const POINTER: Rgba = c(0xe8e6df);
pub const TRACK_DARK: Rgba = c(0x26262b);

/// High-contrast RGB waveform palette. These colors encode frequency content
/// and deliberately remain distinct from the product's interaction accents.
pub const WF_LOW: [f32; 3] = [1.0, 0.22, 0.12];
pub const WF_MID: [f32; 3] = [1.0, 0.68, 0.10];
pub const WF_HIGH: [f32; 3] = [0.26, 0.84, 1.0];
pub const WF_CORE: [f32; 3] = [0.88, 0.98, 1.0];

pub const CUE_COLORS: [Rgba; 8] = [
    c(0xe5484d),
    c(0xff8a3c),
    c(0xffb224),
    c(0xf5d90a),
    c(0xb8e62e),
    c(0x46c46c),
    c(0x2fbfa4),
    c(0xe05fc0),
];

/// Font family resolved from the system (macOS).
pub const FONT_UI: &str = "Helvetica Neue";

pub fn deck_color(deck: mixless_protocol::DeckId) -> Rgba {
    match deck {
        mixless_protocol::DeckId::A => DECK_A,
        mixless_protocol::DeckId::B => DECK_B,
    }
}

pub fn with_alpha(c: impl Into<Hsla>, a: f32) -> Hsla {
    let mut h: Hsla = c.into();
    h.a = a;
    h
}

pub fn cue_color(i: usize) -> Rgba {
    CUE_COLORS[i % CUE_COLORS.len()]
}
