//! Host-only gain preparation. Fit against routed PCM, then validate with
//! the production playback graph. Strategy and musical envelopes are immutable.
use super::*;
#[path = "gain/fit.rs"]
mod fit;
#[path = "gain/meter.rs"]
mod meter;
#[path = "gain/reference.rs"]
mod reference;
#[path = "gain/render.rs"]
mod render;
#[path = "gain/tone.rs"]
mod tone;

const HOP: f32 = 0.2;

#[derive(Clone)]
pub(super) struct Sources {
    // Retain the identities so a replaced buffer cannot reuse its address.
    audio: [Arc<AudioBuffer>; 2],
    stems: [Option<Arc<crate::StemBuffer>>; 2],
}
impl Sources {
    pub(super) fn capture(slots: [&DeckSlot; 2]) -> Option<Self> {
        Some(Self {
            audio: [
                slots[0].buffer.lock().ok()?.clone()?,
                slots[1].buffer.lock().ok()?.clone()?,
            ],
            stems: [
                slots[0]
                    .stems
                    .lock()
                    .ok()?
                    .as_ref()
                    .map(|s| s.audio.clone()),
                slots[1]
                    .stems
                    .lock()
                    .ok()?
                    .as_ref()
                    .map(|s| s.audio.clone()),
            ],
        })
    }
    pub(super) fn matches(&self, slots: [&DeckSlot; 2]) -> bool {
        slots.iter().enumerate().all(|(d, slot)| {
            let Ok(audio) = slot.buffer.lock() else {
                return false;
            };
            let Ok(stems) = slot.stems.lock() else {
                return false;
            };
            audio
                .as_ref()
                .is_some_and(|a| Arc::ptr_eq(a, &self.audio[d]))
                && match (&self.stems[d], stems.as_ref()) {
                    (None, None) => true,
                    (Some(a), Some(b)) => {
                        Arc::ptr_eq(a, &b.audio) && Arc::ptr_eq(&self.audio[d], &b.source)
                    }
                    _ => false,
                }
        })
    }
}

struct Guard<'a> {
    slot: &'a DeckSlot,
    slots: [&'a DeckSlot; 2],
    sources: Sources,
    shared: &'a Shared,
    initial_frame: f64,
    start_frame: f64,
    revision: u64,
    began: std::time::Instant,
}
impl Guard<'_> {
    fn valid(&self) -> bool {
        self.current() && self.began.elapsed().as_secs_f32() < 10.
    }
    fn current(&self) -> bool {
        let now = self.slot.playhead_frames();
        let lead = self.slot.src_sr.load(Ordering::Relaxed) as f64
            * self.slot.rate_micro.load(Ordering::Relaxed) as f64
            / 1_000_000.
            * 0.25;
        self.sources.matches(self.slots)
            && self.shared.automation.gain_revision.load(Ordering::Acquire) == self.revision
            && !(now > self.initial_frame && now + lead > self.start_frame)
    }
}

fn smooth(values: &mut [f32], dt: f32, speed: f32) {
    if let Some(v) = values.first_mut() {
        *v = 0.;
    }
    if let Some(v) = values.last_mut() {
        *v = 0.;
    }
    limit_signed_slope(values, |_| speed * dt);
}

fn limit_signed_slope(values: &mut [f32], edge: impl Fn(usize) -> f32) {
    // Work on magnitudes independently: a positive/negative change must pass
    // through neutral, never average into an unauthorized boost in a gap.
    // Split an edge at its interpolated zero crossing. Giving each sign a
    // whole edge budget would allow twice the intended slew across that edge.
    let original = values.to_vec();
    let budget = |i: usize, at: usize| {
        let (a, b) = (original[i - 1], original[i]);
        if a * b < 0. {
            edge(i) * original[at].abs() / (a.abs() + b.abs())
        } else {
            edge(i)
        }
    };
    for sign in [-1., 1.] {
        let mut magnitude: Vec<_> = original.iter().map(|v| (v * sign).max(0.)).collect();
        if magnitude.is_empty() {
            continue;
        }
        for i in 1..magnitude.len() {
            magnitude[i] = magnitude[i].min(magnitude[i - 1] + budget(i, i));
        }
        for i in (0..magnitude.len() - 1).rev() {
            magnitude[i] = magnitude[i].min(magnitude[i + 1] + budget(i + 1, i));
        }
        for (v, m) in values.iter_mut().zip(magnitude) {
            if *v * sign > 0. {
                *v = sign * m;
            }
        }
    }
}

pub(super) fn compile(
    dense: &mut DensePlan,
    slots: [&DeckSlot; 2],
    shared: &Shared,
    sources: Option<Sources>,
    revision: u64,
) {
    let duration = dense.overlap_frames as f32 / dense.sample_rate as f32;
    if duration < 2. || dense.plan.lanes.scratch_a.is_some() || dense.plan.lanes.scratch_b.is_some()
    {
        return;
    }
    // These live transport modes are not reproduced by the forward preview.
    // Leave the plan intact rather than fit a different signal path.
    if slots.iter().any(|s| {
        s.roll.load(Ordering::Relaxed)
            || s.brake.load(Ordering::Relaxed)
            || s.reverse.load(Ordering::Relaxed)
            || s.loop_on.load(Ordering::Relaxed)
            || s.jog_touch.load(Ordering::Acquire)
            || s.preview_cue.load(Ordering::Acquire) != 0
    }) {
        return;
    }
    let Some(sources) = sources else {
        return;
    };
    let guard = Guard {
        slot: slots[0],
        slots,
        sources,
        shared,
        initial_frame: slots[0].playhead_frames(),
        start_frame: dense.start_frame,
        revision,
        began: std::time::Instant::now(),
    };
    let Some(base) = render::render(dense, slots, shared, false, &guard) else {
        return;
    };
    let Some(mut reference) = render::render(dense, slots, shared, true, &guard) else {
        return;
    };
    let dt = ((HOP * dense.sample_rate as f32) as usize / STEP).max(1) as f32 * STEP as f32
        / dense.sample_rate as f32;
    reference::shape(dense, &mut reference, dt);
    let base_output = base.output;
    let base = base.readings;
    let reference = reference.readings;
    let count = (duration / dt).ceil() as usize;
    if base.len() < count || reference.len() != base.len() {
        return;
    }
    let manual = slots.map(|s| s.limiter_gain_centi.load(Ordering::Relaxed) as f32 / 100. - 12.);
    let mut values: [Vec<f32>; 2] = std::array::from_fn(|_| vec![0.; count + 1]);
    for i in 0..count {
        // Readings end at (i+1)*dt; their momentary window is centered near i*dt.
        let point = &dense.points[((i as f32 * dt * dense.sample_rate as f32) as usize / STEP)
            .min(dense.points.len() - 1)];
        let (a, b) = xfader_gains(point.xf as f32 / 500. - 1., XfCurve::EqualPower, false);
        let routing = [a, b];
        let routing = std::array::from_fn(|d| {
            if d == 1
                && i as f32 * dt < dense.incoming_start_frames as f32 / dense.sample_rate as f32
            {
                0.
            } else {
                routing[d] * point.decks[d].fader as f32 / 1000.
            }
        });
        let gain = fit::point(&base[i], &reference[i], routing, manual);
        for d in 0..2 {
            values[d][i] = gain[d];
        }
    }
    // At most 1.5 dB/s and 0.75 dB per sounding beat. Slow material gets a
    // longer motion without making half/double-time grids the only clock.
    let slowest = dense
        .points
        .iter()
        .map(|p| p.tempo)
        .filter(|v| v.is_finite() && *v > 0.)
        .reduce(f32::min)
        .unwrap_or(120.);
    let slew = (0.75 * slowest / 60.).clamp(0.25, 1.5);
    for v in &mut values {
        smooth(v, dt, slew);
    }
    let gain_changed = values.iter().flatten().any(|v| v.abs() >= 0.05);
    let mut candidate = dense.clone();
    for (j, point) in candidate.points.iter_mut().enumerate() {
        let sec = (j * STEP) as f32 / dense.sample_rate as f32;
        if sec >= duration {
            continue;
        }
        let pos = sec / dt;
        let i = (pos.floor() as usize).min(count - 1);
        for d in 0..2 {
            let db =
                values[d][i] + (values[d][i + 1] - values[d][i]) * (pos - i as f32).clamp(0., 1.);
            let edge = slew * sec.min(duration - sec).max(0.);
            point.decks[d].compensation = (db.clamp(-edge, edge) * 100.).round() as i32;
        }
    }
    let mut best = dense.clone();
    let mut measured = base.clone();
    let mut measured_output = base_output.clone();
    let mut changed = false;
    if gain_changed {
        let proposed = candidate.clone();
        for fraction in [1., 0.5, 0.25] {
            for (point, from) in candidate.points.iter_mut().zip(&proposed.points) {
                for d in 0..2 {
                    point.decks[d].compensation =
                        (from.decks[d].compensation as f32 * fraction).round() as i32;
                }
            }
            let Some(result) = render::render(&candidate, slots, shared, false, &guard) else {
                break;
            };
            if result.readings.len() == base.len()
                && fit::acceptable(&base, &reference, &result.readings)
                && fit::stable_output(&base_output, &result.output)
            {
                best = candidate;
                measured = result.readings;
                measured_output = result.output;
                changed = true;
                break;
            }
        }
    }
    if let Some(candidate) = tone::refine(
        dense,
        &best,
        &base,
        &measured,
        &reference,
        &base_output,
        &measured_output,
        slots,
        shared,
        &guard,
        dt,
        slew,
    ) {
        best = candidate;
        changed = true;
    }
    // An optional refinement that exhausts its budget cannot discard an
    // already validated gain curve. Identity and launch guards still apply.
    if changed && guard.current() {
        dense.points = best.points;
        dense.gain_revision = Some(guard.revision);
        dense.gain_sources = Some(guard.sources);
    }
}

#[cfg(test)]
#[path = "gain/tests.rs"]
mod tests;
