//! Joint phrase exchanges. A dense set of samples in one breath must not hide
//! another breath, and the percussion clock need not own the melodic exchange.
use super::{bar_at, Fade, Shape};
use crate::{score::Tonality, PlanContext};
use mixless_protocol::MixPlan;

/// Closest placement in each distinct rest, keeping the rest's exact measured
/// boundaries. Overlapping placements are one opportunity, not many votes.
pub(super) fn rests(windows: &[Fade], target: f32) -> Vec<Fade> {
    let mut groups: Vec<(f32, Fade)> = Vec::new();
    for &fade in windows {
        if let Some((end, selected)) = groups.last_mut().filter(|g| fade.start <= g.0 + 1e-4) {
            *end = end.max(fade.end);
            if (fade.start - target).abs() < (selected.start - target).abs() {
                *selected = fade;
            }
        } else {
            groups.push((fade.end, fade));
        }
    }
    let mut choices: Vec<_> = groups.into_iter().map(|g| g.1).collect();
    choices.sort_by(|a, b| {
        (a.start - target)
            .abs()
            .total_cmp(&(b.start - target).abs())
    });
    choices.truncate(4);
    choices
}

pub(super) fn shapes(
    windows: &mut crate::melody::Search<'_>,
    p: &MixPlan,
    ctx: &PlanContext<'_>,
    voices: &[Vec<Fade>; 2],
    tonality: Tonality,
    n: f32,
) -> Vec<Shape> {
    let harmonic = tonality == Tonality::Compatible;
    let arrival = crate::drops::peaks(ctx.incoming)
        .iter()
        .filter(|peak| peak.0 >= p.t_in_b - 0.05 && peak.0 < p.t_end_b - 0.05)
        .map(|peak| bar_at(&p.incoming_source, peak.0))
        .fold(n, f32::min);
    let target = p.handoff_bar.unwrap_or(n * 0.5);
    let mut outgoing = windows.windows(true, (target - 2., target));
    outgoing.extend(windows.note_windows(true, (target - 2., target)));
    // Retain different phrase endings; the final routed peak and phrase guards
    // decide whether one can exchange before a coarse section has finished.
    let mut ends: Vec<f32> = Vec::new();
    outgoing.retain(|f| {
        if ends.iter().any(|end| (*end - f.1).abs() < 0.01) {
            true
        } else if ends.len() < 6 {
            ends.push(f.1);
            true
        } else {
            false
        }
    });
    let mut result = Vec::new();
    for (start, end) in outgoing {
        let outgoing_voices = rests(&voices[0], end - 1.);
        for voice_out in outgoing_voices {
            let gate = if harmonic {
                start
            } else {
                end.max(voice_out.end)
            };
            let mut incoming = windows.windows(false, (gate - 1., gate + 1.));
            incoming.extend(windows.note_windows(false, (gate - 1., gate + 1.)));
            incoming.retain(|f| {
                f.0 >= p.incoming_start_bar - 0.001
                    && f.1 <= n + 0.01
                    && (harmonic || f.0 >= gate - 1.5)
            });
            incoming.truncate(8);
            // A foreground opening can precede the nearest exchange rest.
            // Preserve distinct entrances that finish before B's first peak,
            // instead of considering only fades centred on A's melodic gate.
            if arrival < n {
                let mut before_peak = windows.windows(false, (arrival - 2., arrival));
                before_peak.extend(windows.note_windows(false, (arrival - 2., arrival)));
                let mut starts = Vec::<f32>::new();
                for f in before_peak {
                    if f.0 < p.incoming_start_bar - 0.001
                        || f.1 > arrival + 0.001
                        || (!harmonic && f.0 < gate - 1.5)
                        || incoming
                            .iter()
                            .any(|old| (old.0 - f.0).abs() < 0.001 && (old.1 - f.1).abs() < 0.001)
                    {
                        continue;
                    }
                    if !starts.iter().any(|s| (*s - f.0).abs() < 0.001) {
                        if starts.len() == 4 {
                            continue;
                        }
                        starts.push(f.0);
                    }
                    incoming.push(f);
                }
            }
            if !harmonic
                && arrival >= gate
                && crate::vocals::safe_entry(ctx.incoming, p.incoming_source.sample(arrival))
            {
                incoming.push((arrival, arrival));
            }
            for (input_start, input_end) in incoming {
                let voice_gate = if harmonic {
                    voice_out.end
                } else {
                    end.max(voice_out.end)
                };
                let eligible: Vec<_> = voices[1]
                    .iter()
                    .copied()
                    .filter(|f| f.start >= voice_gate - 0.001 && f.end <= n + 0.01)
                    .collect();
                let mut incoming_voices = rests(&eligible, input_end);
                if arrival >= voice_gate
                    && crate::vocals::safe_entry(ctx.incoming, p.incoming_source.sample(arrival))
                {
                    incoming_voices.push(Fade {
                        start: arrival,
                        end: arrival,
                    });
                }
                for voice_in in incoming_voices {
                    // Rhythm may arrive before the foreground. A peak deadline
                    // is one useful anchor; the final energy check is decisive.
                    for (rhythm_out, rhythm_in) in [
                        (start, p.incoming_start_bar),
                        (input_start, (start - 4.).max(p.incoming_start_bar)),
                        (input_start, input_start),
                    ] {
                        let bass = ((end + input_start) * 0.5)
                            .min(arrival - 0.25)
                            .max(p.incoming_start_bar + 0.25);
                        result.push([
                            [
                                voice_out,
                                Fade {
                                    start: rhythm_out,
                                    end: (rhythm_out + 1.).min(n),
                                },
                                Fade { start, end },
                                Fade {
                                    start: bass - 0.25,
                                    end: bass,
                                },
                            ],
                            [
                                voice_in,
                                Fade {
                                    start: rhythm_in,
                                    end: (rhythm_in + 1.).min(n),
                                },
                                Fade {
                                    start: input_start,
                                    end: input_end,
                                },
                                Fade {
                                    start: bass,
                                    end: (bass + 0.25).min(n),
                                },
                            ],
                        ]);
                    }
                }
            }
        }
    }
    result
}
