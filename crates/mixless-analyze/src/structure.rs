//! Bar-scale novelty and recurrence. Fixed 2/4/8-bar kernels use linear
//! feature similarity, so checkerboard novelty is computed from local means
//! without allocating a quadratic self-similarity matrix.
use mixless_protocol::{BarFeature, PhraseBoundary, Section, SectionLabel as S};

mod dynamics;

type Descriptor = [f32; 19];

fn quantile(values: impl Iterator<Item = f32>, fraction: f32) -> f32 {
    let mut values: Vec<_> = values.filter(|v| v.is_finite()).collect();
    values.sort_by(f32::total_cmp);
    values
        .get(((values.len().saturating_sub(1)) as f32 * fraction) as usize)
        .copied()
        .unwrap_or(0.)
}

fn descriptor(bar: &BarFeature, level: f32, attacks: f32) -> Descriptor {
    let mut d = [0.; 19];
    d[0] = (20. * (bar.rms.max(1e-6) / level.max(1e-6)).log10()).clamp(-36., 6.) / 24.;
    let bands =
        [bar.low_db, bar.mid_db, bar.high_db].map(|v| 10f32.powf(v.clamp(-120., 12.) / 10.));
    let total = bands.iter().sum::<f32>().max(1e-12);
    for i in 0..3 {
        d[1 + i] = 0.8 * (bands[i] / total).sqrt();
    }
    d[4] = 0.5 * (bar.onset_density.max(0.).ln_1p() / attacks.max(1.).ln_1p()).min(1.5);
    d[5] = 0.7 * bar.kick_salience.clamp(0., 1.);
    d[6] = 0.5 * bar.vocal_presence.clamp(0., 1.);
    let chroma = bar.chroma.iter().map(|x| x.max(0.)).sum::<f32>().max(1e-6);
    for i in 0..12 {
        d[7 + i] = 0.8 * (bar.chroma[i].max(0.) / chroma).sqrt();
    }
    d
}

fn mean(data: &[Descriptor]) -> Descriptor {
    let mut result = [0.; 19];
    for d in data {
        for i in 0..19 {
            result[i] += d[i] / data.len().max(1) as f32;
        }
    }
    result
}
fn distance(a: &Descriptor, b: &Descriptor) -> f32 {
    a.iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}
fn novelty(data: &[Descriptor], at: usize) -> f32 {
    let mut sum = 0.;
    let mut weight = 0.;
    for (width, w) in [(2, 0.35), (4, 0.4), (8, 0.25)] {
        if at < width || at + width > data.len() {
            continue;
        }
        let (left, right) = (&data[at - width..at], &data[at..at + width]);
        let (a, b) = (mean(left), mean(right));
        let variation = left.iter().map(|d| distance(d, &a)).sum::<f32>() / width as f32
            + right.iter().map(|d| distance(d, &b)).sum::<f32>() / width as f32;
        // A fill increases within-side variance; sustained section changes
        // increase between-side distance. Do not promote every drum accent.
        sum += w * (distance(&a, &b) - 0.6 * variation).max(0.);
        weight += w;
    }
    sum / weight.max(1e-6)
}

pub(crate) fn detect(
    bars: &mut [BarFeature],
    downbeats: &[f32],
) -> (Vec<Section>, Vec<PhraseBoundary>) {
    if bars.is_empty() {
        return (vec![], vec![]);
    }
    // A lone clipped transient must not turn the rest of a quiet recording
    // into silence or decide which bars are the chorus.
    let level = quantile(bars.iter().map(|b| b.rms), 0.8).max(0.001);
    let attacks = quantile(bars.iter().map(|b| b.onset_density), 0.8);
    let mut data: Vec<_> = bars.iter().map(|b| descriptor(b, level, attacks)).collect();
    // Per-bar vocal estimates flicker between voiced and instrumental frames;
    // a 3-bar mean keeps that flicker from inflating within-kernel variation.
    let vocal: Vec<_> = data.iter().map(|d| d[6]).collect();
    for (i, d) in data.iter_mut().enumerate() {
        let lo = i.saturating_sub(1);
        let hi = (i + 2).min(vocal.len());
        d[6] = vocal[lo..hi].iter().sum::<f32>() / (hi - lo) as f32;
    }
    let mut silent: Vec<_> = bars
        .iter()
        .map(|b| b.rms < 0.001 || b.rms < level * 0.02)
        .collect();
    let nov: Vec<_> = (0..bars.len()).map(|i| novelty(&data, i)).collect();
    let median = quantile(nov.iter().copied(), 0.5);
    let mad = quantile(nov.iter().map(|n| (n - median).abs()), 0.5);
    let threshold = (median + 3. * mad).max(0.16);
    let mut peaks: Vec<_> = (2..bars.len().saturating_sub(2))
        .filter(|&i| nov[i] >= threshold && nov[i] >= nov[i - 1] && nov[i] > nov[i + 1])
        .collect();
    peaks.sort_by(|&a, &b| nov[b].total_cmp(&nov[a]));
    let driving = dynamics::driving(bars, level, attacks);
    // Keep a short pre-drop silence attached to an evidenced build. It is
    // tension awaiting resolution, not a new silent section or an exit cue.
    for end in 1..bars.len() {
        if !silent[end - 1] || silent[end] || !driving[end] {
            continue;
        }
        let start = (0..end).rev().find(|&i| !silent[i]).map_or(0, |i| i + 1);
        if end - start <= 2
            && (start.saturating_sub(16)..start).any(|head| {
                mixless_protocol::has_buildup(bars, bars[head].start_sec, bars[end - 1].end_sec)
            })
        {
            silent[start..end].fill(false);
        }
    }
    let mut cuts = vec![0, bars.len()];
    cuts.extend(dynamics::edges(&driving));
    let build_edges = dynamics::build_edges(bars, &driving);
    cuts.extend(&build_edges);
    for i in 1..bars.len() {
        if silent[i] != silent[i - 1] {
            cuts.push(i);
        }
    }
    for i in peaks {
        if cuts.iter().all(|&p| p.abs_diff(i) >= 4) {
            cuts.push(i);
        }
    }
    cuts.sort_unstable();
    cuts.dedup();
    // Novelty peaks tend to fire one bar off the 4-bar phrase grid. When the
    // movable cuts agree on a phase, snap the one-bar stragglers onto it.
    // Cuts at silence edges and the track ends keep their measured position.
    let mut fixed = vec![false; bars.len() + 1];
    fixed[0] = true;
    fixed[bars.len()] = true;
    for i in 1..bars.len() {
        if silent[i] != silent[i - 1] {
            fixed[i] = true;
        }
    }
    let movable: Vec<_> = cuts.iter().copied().filter(|&c| !fixed[c]).collect();
    let mut votes = [0f32; 4];
    for &c in &movable {
        votes[c % 4] += 1. + nov[c].min(1.);
    }
    let phase = (0..4)
        .max_by(|a, b| votes[*a].total_cmp(&votes[*b]))
        .unwrap_or(0);
    if movable.len() >= 3 && votes[phase] >= 0.5 * votes.iter().sum::<f32>() {
        for c in &mut cuts {
            if fixed[*c] {
                continue;
            }
            let target = match (*c + 4 - phase) % 4 {
                1 => *c - 1,
                3 => *c + 1,
                _ => continue,
            };
            if target == 0
                || target >= bars.len()
                || fixed[target]
                || silent[*c - 1]
                || silent[*c]
                || silent[target - 1]
                || silent[target]
            {
                continue;
            }
            *c = target;
        }
        cuts.sort_unstable();
        cuts.dedup();
    }
    let descriptors: Vec<_> = cuts.windows(2).map(|p| mean(&data[p[0]..p[1]])).collect();
    let energies: Vec<_> = cuts
        .windows(2)
        .map(|p| quantile(bars[p[0]..p[1]].iter().map(|b| b.rms), 0.5))
        .collect();
    let first_sound = energies.iter().position(|e| *e >= 0.001).unwrap_or(0);
    let last_sound = energies.iter().rposition(|e| *e >= 0.001).unwrap_or(0);
    let mut sections = Vec::new();
    for (j, p) in cuts.windows(2).enumerate() {
        let selected = &bars[p[0]..p[1]];
        let n = selected.len();
        let rms = energies[j];
        let kick = quantile(selected.iter().map(|b| b.kick_salience), 0.5);
        let foreground = quantile(selected.iter().map(|b| b.vocal_presence), 0.5);
        // A confident human voice helps semantic labels, but a negative model
        // prediction cannot certify that a melody or lead is safe to overlap.
        let voice = quantile(selected.iter().filter_map(|b| b.vocal_confidence), 0.5);
        let voice_known = selected
            .iter()
            .filter(|b| b.vocal_confidence.is_some())
            .count()
            * 4
            >= n * 3;
        let voice_present = if voice_known {
            voice >= 0.4
        } else {
            foreground >= 0.6
        };
        let repeated = descriptors.iter().enumerate().any(|(k, d)| {
            k.abs_diff(j) >= 2
                && cuts[k + 1] - cuts[k] >= 4
                && n >= 4
                && distance(d, &descriptors[j]) < 0.22
        });
        let q = (n / 3).max(1);
        let start_level = quantile(selected[..q].iter().map(|b| b.rms), 0.5);
        let end_level = quantile(selected[n - q..].iter().map(|b| b.rms), 0.5);
        let rising = n >= 4
            && end_level > start_level * 1.35
            && selected
                .windows(2)
                .filter(|p| p[1].rms >= p[0].rms * 0.98)
                .count()
                * 4
                >= (n - 1) * 3;
        let next_louder = energies.get(j + 1).is_some_and(|e| *e > rms * 1.2);
        let driven = driving[p[0]..p[1]].iter().filter(|v| **v).count() * 2 >= n;
        let next_driven = cuts.get(j + 2).is_some_and(|end| {
            driving[p[1]..*end].iter().filter(|v| **v).count() * 2 >= end - p[1]
        });
        let earlier_drop = driving[..p[0]].iter().any(|v| *v);
        let later_drop = driving[p[1]..].iter().any(|v| *v);
        let build =
            mixless_protocol::has_buildup(bars, selected[0].start_sec, selected[n - 1].end_sec);
        // A soft ramp still needs progression over multiple bars. Comparing
        // endpoints alone mistakes the returning kick after a pause for a build.
        let third = (n / 3).max(2).min(n.saturating_sub(1).max(1));
        let tail_end = n.saturating_sub(1).max(1);
        let (head, tail) = (&selected[..third], &selected[tail_end - third..tail_end]);
        let mean_of = |s: &[BarFeature], f: fn(&BarFeature) -> f32| {
            s.iter().map(f).sum::<f32>() / s.len() as f32
        };
        let soft_build = j != first_sound
            && n >= 4
            && n <= 16
            && selected
                .windows(2)
                .filter(|w| {
                    w[1].onset_density > w[0].onset_density.max(0.1) * 1.1
                        || w[1].rms > w[0].rms * 1.05
                        || (w[1].high_db > w[0].high_db + 1.
                            && w[1].high_db - w[1].low_db > w[0].high_db - w[0].low_db + 1.)
                })
                .count()
                * 3
                >= (n - 1) * 2
            && (mean_of(tail, |b| b.onset_density)
                >= mean_of(head, |b| b.onset_density).max(0.1) * 1.25
                || mean_of(tail, |b| b.rms) >= mean_of(head, |b| b.rms) * 1.15
                || (mean_of(tail, |b| b.high_db - b.low_db)
                    >= mean_of(head, |b| b.high_db - b.low_db) + 2.
                    && mean_of(tail, |b| b.high_db) >= mean_of(head, |b| b.high_db) + 1.
                    && mean_of(tail, |b| b.onset_density)
                        >= mean_of(head, |b| b.onset_density).max(1.) * 0.8));
        let label = if silent[p[0]..p[1]].iter().all(|s| *s) {
            S::Silence
        } else if !driven && next_driven && build {
            S::BuildUp
        } else if !driven && next_driven && n >= 2 && soft_build {
            S::BuildUp
        } else if !driven && earlier_drop && later_drop {
            if kick < 0.4 {
                S::Breakdown
            } else {
                S::Break
            }
        } else if !driven && earlier_drop && !later_drop {
            S::Outro
        } else if rising && next_louder && build {
            S::BuildUp
        } else if j == first_sound
            && j < last_sound
            && foreground < 0.45
            && energies[j + 1..].iter().any(|e| *e > rms * 1.3)
        {
            S::Intro
        } else if j == last_sound
            && j > first_sound
            && foreground < 0.45
            && energies[..j].iter().any(|e| *e > rms * 1.3)
        {
            S::Outro
        } else if rms < level * 0.5 && kick < 0.4 {
            S::Breakdown
        } else if repeated && rms > level * 0.8 && voice_present {
            S::Chorus
        } else if voice_known && voice_present && (!driven || voice >= 0.8) {
            // Plosive speech/singing can also excite the low-onset proxy;
            // measured human voice must take precedence over that kick proxy.
            S::Verse
        } else if driven && kick >= 0.5 {
            S::Drop
        } else if voice_present {
            S::Verse
        } else {
            S::Unknown
        };
        let start_sec = selected[0].start_sec;
        let end_sec = selected[n - 1].end_sec;
        for i in p[0]..p[1] {
            bars[i].section = label;
            bars[i].energy_slope = if i > 0 {
                ((bars[i].rms - bars[i - 1].rms) / level).clamp(-1., 1.)
            } else {
                0.
            };
        }
        // Preserve distinct structural boundaries even if both sections have
        // the same semantic label (e.g. two drops with different instrumentation).
        sections.push(Section {
            start_sec,
            end_sec,
            label,
        });
    }
    let mut boundaries = Vec::new();
    for p in cuts.windows(2) {
        let start = bars[p[0]].start_sec;
        let end = bars[p[1] - 1].end_sec;
        let is_silent = silent[p[0]];
        if is_silent {
            continue;
        }
        // Work in actual downbeat coordinates; a pickup bar or leading silence
        // must not shift every subsequent phrase by one bar.
        let first = downbeats.partition_point(|t| *t < start - 0.05);
        if let Some(&t) = downbeats.get(first).filter(|t| (**t - start).abs() <= 0.08) {
            let measured = nov[p[0]];
            let confidence = if p[0] == 0 || silent[p[0] - 1] {
                0.65
            } else {
                (0.6 + measured * 0.3).min(0.95)
            };
            boundaries.push(PhraseBoundary {
                time_sec: t,
                confidence,
                novelty: measured,
            });
            for k in (first + 8..downbeats.len()).step_by(8) {
                let t = downbeats[k];
                if t >= end - 0.05 {
                    break;
                }
                boundaries.push(PhraseBoundary {
                    time_sec: t,
                    confidence: confidence.min(0.65),
                    novelty: 0.,
                });
            }
        }
        if let Some(&t) = downbeats.iter().find(|t| (**t - end).abs() <= 0.08) {
            boundaries.push(PhraseBoundary {
                time_sec: t,
                confidence: 0.65,
                novelty: 0.,
            });
        }
    }
    boundaries.sort_by(|a, b| {
        a.time_sec
            .total_cmp(&b.time_sec)
            .then(b.confidence.total_cmp(&a.confidence))
    });
    boundaries.dedup_by(|a, b| (a.time_sec - b.time_sec).abs() < 0.05);
    (sections, boundaries)
}

#[cfg(test)]
mod melodic_tests;
#[cfg(test)]
mod tests;
