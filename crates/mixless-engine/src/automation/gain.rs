//! Host-only feed-forward compensation. Inspect the actual retained stem PCM
//! and scheduled channel processing; never run a loudness rider in the callback.
use super::*;
use crate::source::{SampleSource, StemSource};

const HOP: f32 = 0.2;
const MAX_DB: f32 = 6.0;
const RISE_DB_PER_SECOND: f32 = 1.5;
const FALL_DB_PER_SECOND: f32 = 4.0;

struct Probe {
    audio: Arc<AudioBuffer>,
    stems: Option<Arc<stems::PreparedStems>>,
    manual_gain: f32,
    resonance: f32,
}

impl Probe {
    fn from_slot(slot: &DeckSlot) -> Option<Self> {
        let audio = slot.buffer.lock().ok()?.clone()?;
        let stems = slot
            .stems
            .lock()
            .ok()?
            .clone()
            .filter(|s| Arc::ptr_eq(&s.source, &audio));
        Some(Self {
            audio,
            stems,
            manual_gain: db_to_lin(
                slot.limiter_gain_centi.load(Ordering::Relaxed) as f32 / 100. - 12.,
            ),
            resonance: if slot.resonance_enabled.load(Ordering::Relaxed) {
                slot.resonance_milli.load(Ordering::Relaxed) as f32 / 1000.
            } else {
                0.
            },
        })
    }

    /// Stereo powers of the whole source and the retained, EQ/filter-shaped
    /// source. Keep cross terms between stems, including the residual, intact.
    fn powers(
        &self,
        sec: f32,
        point: Option<&DeckPoint>,
        loop_range: Option<(u64, u64)>,
    ) -> [f32; 2] {
        let sr = self.audio.sample_rate as f32;
        let gains = point
            .and_then(|p| p.stems)
            .map_or([1.; 3], |s| s.map(|g| g as f32 / 1000.));
        let source = StemSource::new(
            &self.audio,
            self.stems.as_ref().map(|s| s.audio.as_ref()),
            gains,
        );
        let mut eq = [Isolator::new(sr), Isolator::new(sr)];
        let mut filters = [ChannelFilter::new(sr), ChannelFilter::new(sr)];
        if let Some(p) = point {
            for ch in 0..2 {
                eq[ch].gain = p.eq.map(|g| db_to_lin(g as f32 / 100. - 96.));
                eq[ch].set_resonance(self.resonance);
                filters[ch].set_resonance(self.resonance);
                filters[ch].set_amount(sr, p.filter as f32 / 500. - 1.);
            }
        }
        // 80 ms settles the local filters; 400 ms averages over transients.
        let warmup = (0.08 * sr) as usize;
        let frames = (0.48 * sr) as usize;
        let start = ((sec - 0.28) * sr).round() as i64;
        let mut power = [0.0_f64; 2];
        let mut count = 0;
        for j in 0..frames {
            let mut frame = start + j as i64;
            if let Some((start, length)) = loop_range {
                frame = start as i64 + (frame - start as i64).rem_euclid(length.max(1) as i64);
            }
            if frame < 0 || frame >= self.audio.frames as i64 {
                continue;
            }
            let full = self.audio.sample(frame as usize);
            let selected = source.sample(frame as usize);
            for ch in 0..2 {
                let shaped = if point.is_some() {
                    filters[ch].process(eq[ch].process(selected[ch]))
                } else {
                    full[ch]
                };
                if j >= warmup {
                    power[0] += (full[ch] as f64).powi(2);
                    power[1] += (shaped as f64).powi(2);
                }
            }
            count += usize::from(j >= warmup);
        }
        let scale = (self.audio.loudness.gain as f64).powi(2) / (2 * count.max(1)) as f64;
        power.map(|p| (p * scale) as f32)
    }

    fn anchor(&self, start: f32, end: f32) -> f32 {
        let mut powers: Vec<_> = (0..8)
            .map(|i| self.powers(start + (end - start) * (i as f32 + 0.5) / 8., None, None)[0])
            .filter(|p| p.is_finite() && *p > 1e-6)
            .collect();
        if powers.is_empty() {
            return 0.;
        }
        powers.sort_by(f32::total_cmp);
        // Upper median resists a brief drum gap, without targeting a track's drop.
        powers[powers.len() * 5 / 8]
    }
}

fn source_time(plan: &MixPlan, relative: usize, sec: f32) -> (f32, Option<(u64, u64)>) {
    let u = bar_at(&plan.clock, sec);
    let (curve, start, launch, rate, op) = if relative == 0 {
        (
            &plan.outgoing_source,
            plan.t_in_a,
            0.,
            &plan.lanes.rate_a,
            &plan.lanes.loop_a,
        )
    } else {
        (
            &plan.incoming_source,
            plan.t_in_b,
            plan.clock.sample(plan.incoming_start_bar),
            &plan.lanes.rate_b,
            &plan.lanes.loop_b,
        )
    };
    let source = if curve.nodes.is_empty() {
        start + (sec - launch).max(0.) * rate.sample(u)
    } else {
        curve.sample(u)
    };
    let range = op
        .as_ref()
        .filter(|op| u >= op.on_bar && u < op.off_bar)
        .map(|op| (op.start_src_frame, op.length_src_frames));
    (source, range)
}

fn smooth(values: &mut [f32], dt: f32) {
    if values.is_empty() {
        return;
    }
    values[0] = 0.;
    *values.last_mut().unwrap() = 0.;
    for i in 1..values.len() {
        values[i] = values[i].min(values[i - 1] + RISE_DB_PER_SECOND * dt);
    }
    // Look ahead and remove compensation before a drop/stem return, rather
    // than waiting for a limiter to push an unexpectedly louder signal down.
    for i in (0..values.len() - 1).rev() {
        values[i] = values[i].min(values[i + 1] + FALL_DB_PER_SECOND * dt);
    }
}

pub(super) fn compile(
    plan: &MixPlan,
    outgoing: usize,
    shared: &Shared,
    points: &mut [Point],
    sr: u32,
) {
    let Some(summary) = &plan.summary else {
        return;
    };
    let overlap = plan.clock.sample(summary.length_bars as f32);
    if overlap < 2. || plan.lanes.scratch_a.is_some() || plan.lanes.scratch_b.is_some() {
        return;
    }
    let (Some(a), Some(b)) = (
        Probe::from_slot(&shared.decks[outgoing]),
        Probe::from_slot(&shared.decks[1 - outgoing]),
    ) else {
        return;
    };
    let slot = &shared.decks[outgoing];
    let initial_frame = slot.playhead_frames();
    let start_frame = plan.t_in_a as f64 * a.audio.sample_rate as f64;
    let incoming_end = source_time(plan, 1, overlap).0;
    let anchors = [
        a.anchor((plan.t_in_a - 4.).max(0.), plan.t_in_a.max(0.4)),
        b.anchor(
            incoming_end,
            (incoming_end + 4.).min(b.audio.frames as f32 / b.audio.sample_rate as f32),
        ),
    ];
    let probes = [a, b];
    // The clock can include a solo tempo-settling tail after the handoff.
    // Source maps may end at the overlap boundary, so stop compensation here
    // instead of repeatedly probing a frozen source position in that tail.
    let duration = overlap;
    let count = (duration / HOP).ceil().max(1.) as usize;
    let dt = duration / count as f32;
    let mut corrections = Vec::with_capacity(count + 1);
    for i in 0..=count {
        let now = slot.playhead_frames();
        let guard = probes[0].audio.sample_rate as f64
            * slot.rate_micro.load(Ordering::Relaxed) as f64
            / 1_000_000.
            * 0.15;
        if now > initial_frame && now + guard > start_frame {
            // A live deck is approaching the launch. Keep the uncompensated
            // plan rather than delaying the beat for optional gain analysis.
            return;
        }
        let sec = i as f32 * dt;
        let point = &points[((sec * sr as f32) as usize / STEP).min(points.len() - 1)];
        let (xa, xb) = xfader_gains(point.xf as f32 / 500. - 1., XfCurve::EqualPower, false);
        let cross = [xa, xb];
        let mut actual = 0.;
        let mut reference = 0.;
        let mut full = 0.;
        let mut cap = MAX_DB;
        for relative in 0..2 {
            if relative == 1 && sec < plan.clock.sample(plan.incoming_start_bar) {
                continue;
            }
            let p = &point.decks[relative];
            let weight = cross[relative] * p.fader as f32 / 1000.;
            if weight < 0.01 {
                continue;
            }
            let probe = &probes[relative];
            let (time, loop_range) = source_time(plan, relative, sec);
            let powers = probe.powers(time, Some(p), loop_range);
            let gain = weight * db_to_lin(p.gain as f32 / 100. - 96.) * probe.manual_gain;
            actual += powers[1] * gain * gain;
            full += powers[0] * gain * gain;
            reference += anchors[relative] * gain * gain;
            cap = cap.min(12. - 20. * probe.manual_gain.log10());
        }
        // No gain chasing noise, fully muted stems, or deliberate deep gaps.
        // A 1 dB deadband leaves ordinary beat-to-beat variation alone.
        let target = reference.max(full);
        let correction = if actual.is_finite()
            && target.is_finite()
            && actual > 1e-6
            && full > 1e-6
            && actual > target * 0.01
        {
            (10. * (target / actual).log10() - 1.).clamp(0., cap.max(0.))
        } else {
            0.
        };
        corrections.push(correction);
    }
    smooth(&mut corrections, dt);
    for (j, point) in points.iter_mut().enumerate() {
        let position = (j * STEP) as f32 / sr as f32 / dt;
        let i = (position.floor() as usize).min(count - 1);
        let t = (position - i as f32).clamp(0., 1.);
        let db = corrections[i] + (corrections[i + 1] - corrections[i]) * t;
        for deck in &mut point.decks {
            deck.compensation = (db * 100.).round() as u32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compensation_ramps_slowly_and_releases_before_a_return() {
        let mut values = vec![6.; 51];
        values[30] = 0.;
        smooth(&mut values, 0.2);
        assert_eq!(values[0], 0.);
        assert_eq!(values[30], 0.);
        assert_eq!(*values.last().unwrap(), 0.);
        assert!(values.iter().any(|v| *v > 4.));
        for pair in values.windows(2) {
            assert!(pair[1] - pair[0] <= 0.30001);
            assert!(pair[0] - pair[1] <= 0.80001);
        }
    }
}
