//! Display the dry handoff, excluding silent preparation and solo recovery.
//! These are control-envelope windows, not a claim about measured audibility.
use super::{MixPlan, Polyline};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MixWindows {
    pub outgoing: (f32, f32),
    pub incoming: (f32, f32),
}

impl MixPlan {
    /// Source clock for a relative deck (A = outgoing, B = incoming). Unlike
    /// the warp curve, this position wraps while a source loop is playing.
    pub fn source_seconds_at(&self, incoming: bool, u: f32) -> f32 {
        let (source, rate, start, op) = if incoming {
            (
                &self.incoming_source,
                &self.lanes.rate_b,
                self.t_in_b,
                &self.lanes.loop_b,
            )
        } else {
            (
                &self.outgoing_source,
                &self.lanes.rate_a,
                self.t_in_a,
                &self.lanes.loop_a,
            )
        };
        let base = if incoming {
            self.incoming_start_bar
        } else {
            0.
        };
        let unwrapped = |u: f32| match source.nodes.last() {
            None => start + integrate(&self.clock, rate, base, u.max(base)),
            Some(&(last, time)) if u > last => time + integrate(&self.clock, rate, last, u),
            _ => source.sample(u),
        };
        let sr = self.source_sample_rates[usize::from(incoming)] as f32;
        let Some(op) = op.as_ref().filter(|op| sr > 0. && op.length_src_frames > 0) else {
            return unwrapped(u);
        };
        let (start, length) = (
            op.start_src_frame as f32 / sr,
            op.length_src_frames as f32 / sr,
        );
        if u < op.on_bar {
            unwrapped(u)
        } else if u < op.off_bar {
            start + (unwrapped(u) - start).rem_euclid(length)
        } else {
            let repeats = ((unwrapped(op.off_bar) - start - 0.0001) / length)
                .floor()
                .max(0.);
            unwrapped(u) - repeats * length
        }
    }
    /// Source positions occupied by the exchange. Loop windows show the
    /// repeated source region, never the unwrapped transport's fictitious exit.
    pub fn mix_windows(&self) -> Option<MixWindows> {
        let n = self.summary.as_ref()?.length_bars as f32;
        if self.failure_reason.is_some() {
            return None;
        }
        if n == 0. || self.incoming_start_bar >= n {
            return Some(MixWindows {
                outgoing: (self.t_out_a, self.t_out_a),
                incoming: (self.t_in_b, self.t_in_b),
            });
        }
        if self.clock.nodes.len() < 2 {
            return Some(MixWindows {
                outgoing: (self.t_in_a, self.t_out_a),
                incoming: (self.t_in_b, self.t_end_b),
            });
        }
        // Include control vertices as well as a bounded sampling grid. Short
        // independent stem ramps must not disappear between display samples.
        let mut points: Vec<_> = (0..=(n * 16.).ceil() as usize)
            .map(|i| (i as f32 / 16.).min(n))
            .collect();
        points.push(self.incoming_start_bar);
        for line in [&self.lanes.xfader, &self.lanes.gain_a, &self.lanes.gain_b]
            .into_iter()
            .chain(
                self.stem_mix
                    .iter()
                    .flat_map(|s| s.outgoing.iter().chain(&s.incoming)),
            )
        {
            points.extend(line.nodes.iter().map(|p| p.0).filter(|u| *u <= n));
        }
        points.sort_by(f32::total_cmp);
        points.dedup();
        let level = |incoming: bool, u: f32| {
            let x = ((self.lanes.xfader.sample(u) + 1.) * 0.5).clamp(0., 1.);
            let (side, db, parts) = if incoming {
                (
                    x,
                    &self.lanes.gain_b,
                    self.stem_mix.as_ref().map(|s| &s.incoming),
                )
            } else {
                (
                    1. - x,
                    &self.lanes.gain_a,
                    self.stem_mix.as_ref().map(|s| &s.outgoing),
                )
            };
            let gain = side.sqrt() * 10f32.powf(db.sample(u) / 20.);
            let stems = parts.map_or([1.; 4], |p| p.each_ref().map(|p| p.sample(u)));
            stems.map(|s| s * gain)
        };
        let reference_a = level(false, 0.);
        let reference_b = level(true, n);
        let entry = points
            .windows(2)
            .find(|w| {
                w[1] >= self.incoming_start_bar && level(true, w[1]).iter().any(|v| *v > 0.001)
            })
            .map_or(n, |w| w[0].max(self.incoming_start_bar));
        let departure = points
            .windows(2)
            .find(|w| {
                level(false, w[1])
                    .iter()
                    .zip(reference_a)
                    .any(|(v, initial)| initial > 0.001 && *v < initial * 0.99)
            })
            .map_or(entry, |w| w[0])
            .min(entry);
        let exit = points
            .windows(2)
            .rev()
            .find(|w| level(false, w[0]).iter().any(|v| *v > 0.001))
            .map_or(0., |w| w[1]);
        let established = points
            .windows(2)
            .rev()
            .find(|w| {
                level(true, w[0])
                    .iter()
                    .zip(reference_b)
                    .any(|(v, final_level)| final_level > 0.001 && *v < final_level * 0.99)
            })
            .map_or(entry, |w| w[1]);
        Some(MixWindows {
            outgoing: self.source_window(false, departure, exit.max(departure)),
            incoming: self.source_window(true, entry, established.max(exit).max(entry)),
        })
    }

    fn source_window(&self, incoming: bool, from: f32, to: f32) -> (f32, f32) {
        let index = usize::from(incoming);
        let (start, loop_op) = if incoming {
            (self.t_in_b, &self.lanes.loop_b)
        } else {
            (self.t_in_a, &self.lanes.loop_a)
        };
        let Some(op) = loop_op
            .as_ref()
            .filter(|_| self.source_sample_rates[index] > 0)
        else {
            // Older serialized loop plans do not have sample rates. Their
            // stored source window is safer than displaying unwrapped bars.
            return if loop_op.is_some() {
                (start, if incoming { self.t_end_b } else { self.t_out_a })
            } else {
                (
                    self.source_seconds_at(incoming, from),
                    self.source_seconds_at(incoming, to),
                )
            };
        };
        let sr = self.source_sample_rates[index] as f32;
        let loop_start = op.start_src_frame as f32 / sr;
        let length = op.length_src_frames as f32 / sr;
        if length <= 0. {
            return (start, start);
        }
        let at = |u| self.source_seconds_at(incoming, u);
        let (a, b) = (at(from), at(to));
        if from < op.off_bar && to > op.on_bar {
            (a.min(b).min(loop_start), a.max(b).max(loop_start + length))
        } else {
            (a.min(b), a.max(b))
        }
    }
}

fn integrate(clock: &Polyline, rate: &Polyline, from: f32, to: f32) -> f32 {
    let mut nodes = vec![from, to];
    nodes.extend(
        clock
            .nodes
            .iter()
            .chain(&rate.nodes)
            .map(|p| p.0)
            .filter(|u| *u > from && *u < to),
    );
    nodes.sort_by(f32::total_cmp);
    nodes.dedup();
    nodes
        .windows(2)
        .map(|w| {
            (clock.sample(w[1]) - clock.sample(w[0]))
                * (rate.sample(w[0]) + rate.sample(w[1]))
                * 0.5
        })
        .sum()
}

#[cfg(test)]
mod tests;
