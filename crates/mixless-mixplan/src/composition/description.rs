//! Describe the fitted part schedule, including a silent incoming lead-in.
use super::{Frame, Shape};
use mixless_protocol::{MixStage, StrategyId};

pub(super) fn describe(frames: &[Frame], shape: &Shape, n: f32) -> (StrategyId, Vec<MixStage>) {
    let occupancy = super::occupancy::measure(frames, shape, n);
    let overlap = occupancy.overlap_bars > 0.;
    // Both decks must hold selected, audible parts for a complete bar. Tiny
    // timing differences, a single fade, and full-track layering are blends.
    let independent = occupancy.selected_run >= 1.;
    let vocal_carry = occupancy.carried_voice && shape[0][0].start > shape[1][2].end + 0.5;
    let fades = shape.iter().flatten().any(|f| f.end - f.start > 0.0001);
    let strategy = if vocal_carry {
        StrategyId::VocalRide
    } else if independent {
        StrategyId::StemSwap
    } else if overlap || fades {
        StrategyId::PhraseBlend
    } else {
        StrategyId::DryCut
    };
    let entry = shape[1].iter().map(|f| f.start).fold(n, f32::min);
    let stage = |start_bar, end_bar, label: &str| MixStage {
        start_bar,
        end_bar,
        label: label.into(),
    };
    let mut stages = vec![stage(0., entry, "Wait for incoming entry")];
    if independent || vocal_carry {
        stages.extend([
            stage(entry, shape[1][3].start, "Layer selected parts"),
            stage(
                shape[1][3].start.max(entry),
                shape[1][3].end,
                "Exchange bass",
            ),
            stage(
                shape[1][3].end.max(entry),
                n,
                if vocal_carry {
                    "Carry vocal to its release"
                } else {
                    "Exchange foreground and release layer"
                },
            ),
        ]);
    } else {
        stages.push(stage(
            entry,
            n,
            if overlap {
                "Crossfade tracks"
            } else if fades {
                "Fade between tracks"
            } else {
                "Switch tracks"
            },
        ));
    }
    stages.retain(|s| s.end_bar > s.start_bar);
    (strategy, stages)
}
