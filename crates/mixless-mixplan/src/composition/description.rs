//! Describe the fitted part schedule, including a silent incoming lead-in.
use super::{gains, Frame, Shape};
use mixless_protocol::{MixStage, StrategyId};

pub(super) fn describe(frames: &[Frame], shape: &Shape, n: f32) -> (StrategyId, Vec<MixStage>) {
    // Inspect every fade boundary and interval, so a short exchange cannot
    // disappear between the evidence grid's samples. Ignore only numerical
    // zero and parts below the same relative -40 dB floor as voice evidence.
    let mut points = vec![0., n];
    points.extend(shape.iter().flatten().flat_map(|f| [f.start, f.end]));
    points.sort_by(f32::total_cmp);
    points.dedup();
    let mut overlap = false;
    let mut independent = false;
    let mut carried_voice = false;
    for u in points
        .iter()
        .copied()
        .chain(points.windows(2).map(|w| (w[0] + w[1]) / 2.))
    {
        let g = gains(shape, u);
        let f = &frames[((u / n * 128.).round() as usize).min(128)];
        let levels: [[f32; 4]; 2] = std::array::from_fn(|d| {
            let power = f.bands[d].map(|bands| bands.iter().sum::<f32>());
            let total = power.iter().sum::<f32>();
            std::array::from_fn(|s| {
                if power[s] > total * 0.0001 {
                    g[d][s]
                } else {
                    0.
                }
            })
        });
        if levels.iter().any(|deck| deck.iter().all(|g| *g <= 0.0001)) {
            continue;
        }
        overlap = true;
        carried_voice |= f.voice[0] > 0.35
            && levels[0][0] > 0.0001
            && levels[1][2] > 0.0001
            && g[0][2] <= 0.0001;
        // Comparing the desired gains alone would count silent vocals as an
        // independent layer. Compare only parts with measured source power.
        independent |= (0..2).any(|d| {
            let power = f.bands[d].map(|bands| bands.iter().sum::<f32>());
            let total = power.iter().sum::<f32>();
            (0..4).any(|a| {
                power[a] > total * 0.0001
                    && (0..4)
                        .any(|b| power[b] > total * 0.0001 && (g[d][a] - g[d][b]).abs() > 0.0001)
            })
        });
    }
    let vocal_carry = carried_voice && shape[0][0].start > shape[1][2].end + 0.5;
    let fades = shape.iter().flatten().any(|f| f.end - f.start > 0.0001);
    let strategy = if independent && vocal_carry {
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
    if independent {
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
