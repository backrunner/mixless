//! Choose the nearest phrase exchange that fits key, voice and peak constraints.
use super::{bar_at, Fade, Shape};
use crate::{score::Tonality, PlanContext};
use mixless_protocol::MixPlan;

pub(super) fn shapes(
    windows: &mut crate::melody::Search<'_>,
    p: &MixPlan,
    ctx: &PlanContext<'_>,
    shape: Shape,
    voices: &[Fade],
    tonality: Tonality,
    protected: f32,
    n: f32,
) -> Vec<Shape> {
    let harmonic = tonality == Tonality::Compatible;
    // Without a confirmed key the incoming melody may start under the last
    // outgoing bars; the fitted cost limits their shared audible time.
    let lead = if tonality == Tonality::Unknown {
        2.
    } else {
        1.
    };
    let arrival = crate::drops::peaks(ctx.incoming)
        .iter()
        .filter(|peak| peak.0 >= p.t_in_b - 0.05 && peak.0 < p.t_end_b - 0.05)
        .map(|peak| bar_at(&p.incoming_source, peak.0))
        .fold(n, f32::min);
    let out = shape[0][2];
    let mut result = Vec::new();
    let structural = windows.windows(true, (out.start, out.end));
    let notes = windows.note_windows(true, (out.start, out.end));
    for (start, end) in [structural, notes]
        .into_iter()
        .flat_map(|fs| fs.into_iter().filter(|f| f.1 >= protected - 0.05).take(4))
    {
        let gate = if harmonic {
            0.
        } else {
            shape[0][0].end.max(end)
        };
        // Search around the fitted outgoing release, not the original search
        // seed. Otherwise its four nearest entrances can all leave a long
        // drum-only hole even when an adjacent melodic exchange is feasible.
        let arrival_start = if harmonic {
            start
        } else {
            (gate - lead).max(0.)
        };
        let incoming = windows.windows(false, (arrival_start, (arrival_start + 4.).min(n)));
        // A four-bar reveal may miss a drop where the two-bar alternative fits.
        // Test feasibility before truncating, not after committing to one width.
        let notes = windows.note_windows(false, (arrival_start, (arrival_start + 4.).min(n)));
        // Across keys, B's kit can carry A's release and B's foreground then
        // lands whole on the arrival downbeat: its peak, or the plan's end.
        let landing = (!harmonic && arrival >= gate.max(p.incoming_start_bar) - 0.001)
            .then_some((arrival, arrival));
        // Compatible keys may reveal B anywhere; otherwise B waits for the lead.
        let earliest = if harmonic { gate } else { arrival_start };
        for (input_start, input_end) in [incoming, notes]
            .into_iter()
            .flat_map(|fs| {
                fs.into_iter()
                    .filter(|f| {
                        f.0 >= earliest.max(p.incoming_start_bar) - 0.001 && f.1 <= arrival + 0.01
                    })
                    .take(4)
            })
            .chain(landing)
        {
            let mut candidate = shape;
            candidate[0][2] = Fade { start, end };
            candidate[1][2] = Fade {
                start: input_start,
                end: input_end,
            };
            if !harmonic {
                // Two singers never overlap. With unknown keys the cost bounds
                // a singer over the other deck's melody; conflicts wait for it.
                let voice_gate = if tonality == Tonality::Unknown {
                    shape[0][0].end.max(gate - 1.)
                } else {
                    gate
                };
                // A landing singer must start a complete line on that downbeat.
                let landed = (input_start >= arrival - 0.001
                    && crate::vocals::safe_entry(ctx.incoming, p.incoming_source.sample(arrival)))
                .then_some(Fade {
                    start: arrival,
                    end: arrival,
                });
                let Some(voice) = voices
                    .iter()
                    .copied()
                    .find(|f| f.start >= voice_gate - 0.001 && f.end <= arrival + 0.01)
                    .or(landed)
                else {
                    continue;
                };
                candidate[1][0] = voice;
                candidate[0][3] = Fade {
                    start: (gate - 0.25).max(0.),
                    end: gate,
                };
                candidate[1][3] = Fade {
                    start: gate,
                    end: (gate + 0.25).min(n),
                };
            }
            result.push(candidate);
        }
    }
    // Across keys, B's kit can carry A's whole release; the tonal parts then
    // trade in one complementary bar so B's foreground lands on the arrival
    // downbeat (its peak, or the plan end). A slower fade would leave a gap.
    // Both fades stay audibly longer than the 3/4-bar release minimum while
    // their overlap above -40 dB remains under one bar.
    let swap = arrival - 1.5;
    if !harmonic
        && swap >= p.incoming_start_bar.max(0.25)
        && arrival >= protected - 0.05
        && shape[0][0].end <= swap + 0.001
        && crate::vocals::safe_entry(ctx.incoming, p.incoming_source.sample(arrival))
    {
        let mut candidate = shape;
        candidate[0][2] = Fade {
            start: swap,
            end: arrival - 0.25,
        };
        candidate[1][2] = Fade {
            start: arrival - 1.25,
            end: arrival,
        };
        let bass = arrival - 0.75;
        candidate[1][0] = voices
            .iter()
            .copied()
            .find(|f| f.start >= swap - 0.001 && f.end <= arrival + 0.01)
            .unwrap_or(Fade {
                start: arrival,
                end: arrival,
            });
        candidate[0][3] = Fade {
            start: bass - 0.25,
            end: bass,
        };
        candidate[1][3] = Fade {
            start: bass,
            end: bass + 0.25,
        };
        result.push(candidate);
    }
    result
}
