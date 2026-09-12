//! Explicit technique selection for the smooth planner.
//!
//! FX is a musical repair or a short accent, never a score multiplier.  The
//! policy therefore applies hard vetoes before ranking: a long vocal overlap,
//! an uncertain grid, or no phrase/downbeat evidence can only select a dry
//! phrase handoff or a conservative bridge.

use mixless_protocol::SectionLabel;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Technique {
    DryCut,
    EchoOut,
    FilterBridge,
    DropCut,
    LoopRoll,
    ScratchCut,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Evidence {
    pub loop_roll: bool,
    pub scratch: bool,
    pub drop: bool,
    pub grid_reliable: bool,
    pub phrase_reliable: bool,
    pub harmonic_compatible: bool,
    pub vocal_overlap: f32,
    pub outgoing_kick: f32,
    pub incoming_kick: f32,
    pub outgoing_vocal: f32,
    pub incoming_vocal: f32,
    pub outgoing_section: SectionLabel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Decision {
    pub technique: Technique,
    pub confidence: f32,
}

pub(crate) fn choose(e: Evidence) -> Decision {
    let vocal_overlap = e.vocal_overlap.clamp(0.0, 1.0);
    let strong_kicks = e.outgoing_kick >= 0.55 && e.incoming_kick >= 0.55;
    let clean_foregrounds =
        e.outgoing_vocal < 0.35 && e.incoming_vocal < 0.65 && vocal_overlap <= 0.0625;

    // Repetition must be rhythmically exact. Never use a loop as camouflage
    // for an unreliable grid or an exposed vocal.
    if e.loop_roll && e.grid_reliable && e.phrase_reliable && clean_foregrounds && strong_kicks {
        return Decision {
            technique: Technique::LoopRoll,
            confidence: 0.94,
        };
    }

    // Scratch is an accent, not a rescue. It needs explicit cue evidence,
    // clean foregrounds and a stable handoff; the planner computes `scratch`
    // only after checking the user Hot cue and source bounds.
    if e.scratch && e.grid_reliable && e.phrase_reliable && clean_foregrounds && strong_kicks {
        return Decision {
            technique: Technique::ScratchCut,
            confidence: 0.91,
        };
    }

    if e.drop && e.phrase_reliable && e.incoming_kick >= 0.5 && e.incoming_vocal < 0.75 {
        return Decision {
            technique: Technique::DropCut,
            confidence: 0.88,
        };
    }

    // A harmonic, percussive and phrase-aligned handoff needs no FX. Adding
    // echo/filter here would blur a clean downbeat and reduce transient punch.
    if e.grid_reliable && e.phrase_reliable && e.harmonic_compatible && clean_foregrounds {
        return Decision {
            technique: Technique::DryCut,
            confidence: 0.84,
        };
    }

    // Echo fills a short gap when the outgoing side is ending or a vocal is
    // present. Filter is reserved for incompatible tonal material where an
    // echo tail would repeat the wrong foreground.
    if matches!(
        e.outgoing_section,
        SectionLabel::Outro | SectionLabel::Break
    ) || e.outgoing_vocal >= 0.35
    {
        Decision {
            technique: Technique::EchoOut,
            confidence: 0.72,
        }
    } else {
        Decision {
            technique: Technique::FilterBridge,
            confidence: 0.65,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Evidence {
        Evidence {
            loop_roll: false,
            scratch: false,
            drop: false,
            grid_reliable: true,
            phrase_reliable: true,
            harmonic_compatible: true,
            vocal_overlap: 0.0,
            outgoing_kick: 0.8,
            incoming_kick: 0.8,
            outgoing_vocal: 0.1,
            incoming_vocal: 0.1,
            outgoing_section: SectionLabel::Intro,
        }
    }

    #[test]
    fn clean_harmonic_handoff_stays_dry() {
        assert_eq!(choose(base()).technique, Technique::DryCut);
    }

    #[test]
    fn loop_has_priority_only_with_clean_rhythm() {
        let mut e = base();
        e.loop_roll = true;
        assert_eq!(choose(e).technique, Technique::LoopRoll);
        e.incoming_vocal = 0.9;
        assert_ne!(choose(e).technique, Technique::LoopRoll);
    }

    #[test]
    fn scratch_is_vetoed_without_stable_grid() {
        let mut e = base();
        e.scratch = true;
        e.grid_reliable = false;
        assert_ne!(choose(e).technique, Technique::ScratchCut);
    }

    #[test]
    fn outro_with_foreground_prefers_echo() {
        let mut e = base();
        e.outgoing_section = SectionLabel::Outro;
        e.outgoing_vocal = 0.7;
        assert_eq!(choose(e).technique, Technique::EchoOut);
    }
}
