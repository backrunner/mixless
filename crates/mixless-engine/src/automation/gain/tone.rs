//! Host-only refinement of the same transition. Probes measure the response
//! of the real graph; the local model is only a proposal, never the validator.
use super::*;
use meter::Reading;

#[path = "tone_envelope.rs"]
mod envelope;
#[path = "tone_model.rs"]
mod model;

pub(super) struct Probe {
    pub lane: usize,
    pub readings: Vec<Reading>,
}

fn needs_tone(reading: &Reading, reference: &Reading) -> bool {
    let target = fit::target(reference, 0);
    if target < 1e-6 || reading[2].power[0] < 1e-6 {
        return false;
    }
    let wide = model::ratio(reading[2].power[0], target);
    let largest = (1..4)
        .map(|b| fit::target(reference, b))
        .fold(0f64, f64::max);
    (1..4).any(|b| {
        let target = fit::target(reference, b);
        target > largest * 0.01
            && target > 1e-8
            && (model::ratio(reading[2].power[b], target) - wide).abs() > 1.
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn refine(
    original: &DensePlan,
    best: &DensePlan,
    dry: &[Reading],
    base: &[Reading],
    reference: &[Reading],
    dry_output: &render::Output,
    base_output: &render::Output,
    slots: [&DeckSlot; 2],
    shared: &Shared,
    guard: &Guard<'_>,
    dt: f32,
    slew: f32,
) -> Option<DensePlan> {
    if !guard.valid() || !base.iter().zip(reference).any(|(b, r)| needs_tone(b, r)) {
        return None;
    }
    let mut probes = Vec::new();
    let mut cost = guard.began.elapsed().as_secs_f32() / 3.;
    // Stems first: when time is scarce, prefer a measured source-specific
    // correction. EQ probes remain available for mixtures and missing stems.
    for lane in [3, 4, 5, 6, 0, 1, 2] {
        if guard.began.elapsed().as_secs_f32() + cost * 2.2 >= 10. {
            break; // Reserve one complete validation render.
        }
        if lane >= 3
            && !slots.iter().any(|s| {
                s.stems
                    .lock()
                    .ok()
                    .is_some_and(|v| v.as_ref().is_some_and(|v| lane != 6 || v.audio.has_bass()))
            })
        {
            continue;
        }
        let mut probe = best.clone();
        let mut available = false;
        for (i, point) in probe.points.iter_mut().enumerate() {
            if i * STEP >= probe.overlap_frames {
                break;
            }
            for deck in &mut point.decks {
                if envelope::level(deck, lane) > -40. {
                    deck.tone_compensation[lane] = -100;
                    available = true;
                }
            }
        }
        if !available {
            continue;
        }
        let began = std::time::Instant::now();
        let reading = render::render(&probe, slots, shared, false, guard)?;
        cost = began.elapsed().as_secs_f32();
        if reading.readings.len() != base.len() {
            return None;
        }
        probes.push(Probe {
            lane,
            readings: reading.readings,
        });
    }
    if probes.is_empty() || !guard.valid() {
        return None;
    }
    let duration = original.overlap_frames as f32 / original.sample_rate as f32;
    let count = (duration / dt).ceil() as usize;
    let manual = slots.map(|s| s.limiter_gain_centi.load(Ordering::Relaxed) as f32 / 100. - 12.);
    let mut curves = vec![[[0.; 8]; 2]; count + 1];
    for i in 0..count {
        if !guard.valid() {
            return None;
        }
        let at = ((i as f32 * dt * original.sample_rate as f32) as usize / STEP)
            .min(original.points.len() - 1);
        curves[i] = model::solve(
            &original.points[at],
            &best.points[at],
            &dry[i],
            &base[i],
            &reference[i],
            &probes,
            i,
            manual,
        );
    }
    let mut candidate = best.clone();
    envelope::apply(original, &mut candidate, &curves, dt, slew);
    if !candidate.points.iter().any(|p| {
        p.decks
            .iter()
            .any(|d| d.tone_compensation.iter().any(|v| v.abs() >= 5))
    }) {
        return None;
    }
    let proposed = candidate.clone();
    let wide_error = |readings: &[Reading]| -> f64 {
        readings
            .iter()
            .zip(reference)
            .map(|(b, r)| {
                if fit::target(r, 0) < 1e-6 {
                    0.
                } else {
                    (model::ratio(b[2].power[0], fit::target(r, 0)).abs() - 1.)
                        .max(0.)
                        .powi(2)
                }
            })
            .sum()
    };
    // Back off toward the already accepted result when the local model is
    // optimistic. Every attempted curve still needs a complete real render.
    for fraction in [1., 0.5, 0.25] {
        for ((point, from), to) in candidate
            .points
            .iter_mut()
            .zip(&best.points)
            .zip(&proposed.points)
        {
            for d in 0..2 {
                let blend = |a: i32, b: i32| (a as f32 + (b - a) as f32 * fraction).round() as i32;
                point.decks[d].compensation =
                    blend(from.decks[d].compensation, to.decks[d].compensation);
                point.decks[d].tone_compensation = std::array::from_fn(|k| {
                    blend(
                        from.decks[d].tone_compensation[k],
                        to.decks[d].tone_compensation[k],
                    )
                });
            }
        }
        let result = render::render(&candidate, slots, shared, false, guard)?;
        let rendered = result.readings;
        if rendered.len() != base.len() || !guard.valid() {
            return None;
        }
        // Spectral improvement cannot buy a larger broadband mismatch. Check
        // against both the accepted gain curve and the untouched original.
        if wide_error(&rendered) <= wide_error(base) + 1e-6
            && fit::validate(base, reference, &rendered, true)
            && fit::validate(dry, reference, &rendered, true)
            && fit::stable_output(dry_output, &result.output)
            && fit::stable_output(base_output, &result.output)
        {
            return Some(candidate);
        }
    }
    None
}
