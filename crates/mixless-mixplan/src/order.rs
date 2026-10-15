//! Playlist order for the smoothest set: an open path through every track
//! that keeps each neighbour within reach of a beat blend, harmonically close
//! and without abrupt level changes. Pure and deterministic.
use mixless_protocol::TrackAnalysis;

/// Cost of mixing `a` into `b`; 0 is an ideal pair, about 1 needs a
/// native-rate handoff, and each part only grows with the musical distance.
pub fn transition_cost(a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    tempo_cost(a, b) + key_cost(a, b) + energy_cost(a, b)
}

/// Every index once, starting at `first` when given. Tracks without a
/// measured tempo keep their relative order at the end.
pub fn best_order(tracks: &[&TrackAnalysis], first: Option<usize>) -> Vec<usize> {
    let measured: Vec<usize> = (0..tracks.len())
        .filter(|i| Some(*i) == first || tracks[*i].tempo.global_bpm > 0.)
        .collect();
    let rest: Vec<usize> = (0..tracks.len())
        .filter(|i| !measured.contains(i))
        .collect();
    let n = measured.len();
    if n < 3 {
        return measured.into_iter().chain(rest).collect();
    }
    let cost: Vec<Vec<f32>> = measured
        .iter()
        .map(|&i| {
            measured
                .iter()
                .map(|&j| transition_cost(tracks[i], tracks[j]))
                .collect()
        })
        .collect();
    let length = |path: &[usize]| path.windows(2).map(|w| cost[w[0]][w[1]]).sum::<f32>();
    let starts: Vec<usize> = match first.and_then(|f| measured.iter().position(|i| *i == f)) {
        Some(start) => vec![start],
        None => (0..n).collect(),
    };
    let mut best: Option<(f32, Vec<usize>)> = None;
    for start in starts {
        let mut path = nearest_neighbour(&cost, start);
        improve(&cost, &mut path);
        let total = length(&path);
        if best.as_ref().is_none_or(|(b, _)| total < *b - 1e-6) {
            best = Some((total, path));
        }
    }
    let path = best.map(|(_, p)| p).unwrap_or_default();
    path.into_iter().map(|i| measured[i]).chain(rest).collect()
}

fn nearest_neighbour(cost: &[Vec<f32>], start: usize) -> Vec<usize> {
    let n = cost.len();
    let mut path = vec![start];
    let mut used = vec![false; n];
    used[start] = true;
    while path.len() < n {
        let last = *path.last().unwrap();
        let next = (0..n)
            .filter(|j| !used[*j])
            .min_by(|x, y| cost[last][*x].total_cmp(&cost[last][*y]))
            .unwrap();
        used[next] = true;
        path.push(next);
    }
    path
}

/// 2-opt with the first track fixed, then single-track relocation, until no
/// move shortens the path. Costs are symmetric, so a reversal keeps pairs.
fn improve(cost: &[Vec<f32>], path: &mut Vec<usize>) {
    let n = path.len();
    let edge = |p: &[usize], i: usize| {
        if i + 1 < p.len() {
            cost[p[i]][p[i + 1]]
        } else {
            0.
        }
    };
    for _ in 0..64 {
        let mut changed = false;
        for i in 1..n - 1 {
            for j in i + 1..n {
                let before = cost[path[i - 1]][path[i]] + edge(path, j);
                let after = cost[path[i - 1]][path[j]]
                    + if j + 1 < n {
                        cost[path[i]][path[j + 1]]
                    } else {
                        0.
                    };
                if after < before - 1e-6 {
                    path[i..=j].reverse();
                    changed = true;
                }
            }
        }
        for from in 1..n {
            let track = path[from];
            let removed_gain = cost[path[from - 1]][track] + edge(path, from)
                - if from + 1 < n {
                    cost[path[from - 1]][path[from + 1]]
                } else {
                    0.
                };
            let mut rest = path.clone();
            rest.remove(from);
            let mut best = (0f32, from);
            for to in 1..=rest.len() {
                let added = cost[rest[to - 1]][track]
                    + if to < rest.len() {
                        cost[track][rest[to]] - cost[rest[to - 1]][rest[to]]
                    } else {
                        0.
                    };
                if removed_gain - added > best.0 + 1e-6 {
                    best = (removed_gain - added, to);
                }
            }
            if best.0 > 0. {
                rest.insert(best.1, track);
                *path = rest;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
}

/// Smallest log tempo distance allowing half/double time. Within 8% a beat
/// blend is direct, within 16% it needs a silent tempo preparation, beyond
/// that only a native-rate handoff remains.
fn tempo_cost(a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    let (x, y) = (a.tempo.global_bpm, b.tempo.global_bpm);
    if x <= 0. || y <= 0. {
        return 1.;
    }
    let d = [0.5f32, 1., 2.]
        .into_iter()
        .map(|f| (x * f / y).ln().abs())
        .fold(f32::INFINITY, f32::min);
    let (blend, prepared) = (1.08f32.ln(), 1.16f32.ln());
    if d <= blend {
        0.3 * d / blend
    } else if d <= prepared {
        0.3 + 0.5 * (d - blend) / (prepared - blend)
    } else {
        1. + (d - prepared)
    }
}

/// Camelot compatibility, including the two-semitone shift a blend may apply.
/// Uncertain keys are neither rewarded nor punished.
fn key_cost(a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    let (compatible, _) = crate::score::key_match(a, b, 0., 0.);
    let certainty = a.key_confidence.min(b.key_confidence).clamp(0., 1.);
    0.5 * (1. - compatible) * certainty + 0.25 * (1. - certainty)
}

/// Typical level of the sounding bars, in dB.
fn loudness(t: &TrackAnalysis) -> Option<f32> {
    let mut rms: Vec<f32> = t
        .bars
        .iter()
        .map(|b| b.rms)
        .filter(|r| r.is_finite() && *r > 0.001)
        .collect();
    rms.sort_by(f32::total_cmp);
    rms.get(rms.len() / 2).map(|r| 20. * r.log10())
}

fn energy_cost(a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    match (loudness(a), loudness(b)) {
        (Some(x), Some(y)) => ((x - y).abs() / 6.).min(1.) * 0.3,
        _ => 0.,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::SectionLabel as L;

    fn track(id: i64, bpm: f32, key: &str) -> TrackAnalysis {
        crate::tests::track(id, bpm, key, L::Drop, 32, 0.9, 0.2)
    }

    #[test]
    fn tempo_islands_are_joined_once_and_keys_chain() {
        let tracks = [
            track(1, 174., "8A"),
            track(2, 120., "8A"),
            track(3, 175., "9A"),
            track(4, 122., "9A"),
            track(5, 172., "8A"),
            track(6, 124., "10A"),
        ];
        let refs: Vec<_> = tracks.iter().collect();
        let order = best_order(&refs, Some(0));
        assert_eq!(order[0], 0);
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..6).collect::<Vec<_>>());
        let jumps = order
            .windows(2)
            .filter(|w| tempo_cost(&tracks[w[0]], &tracks[w[1]]) >= 1.)
            .count();
        assert_eq!(jumps, 1, "{order:?}");
    }

    #[test]
    fn half_time_counts_as_the_same_tempo() {
        assert!(tempo_cost(&track(1, 174., "8A"), &track(2, 87., "8A")) < 0.01);
        assert!(tempo_cost(&track(1, 174., "8A"), &track(2, 140., "8A")) >= 1.);
    }

    #[test]
    fn order_is_deterministic_without_a_fixed_start() {
        let tracks: Vec<_> = [128., 140., 126., 174., 130., 172.]
            .iter()
            .enumerate()
            .map(|(i, bpm)| track(i as i64, *bpm, "5A"))
            .collect();
        let refs: Vec<_> = tracks.iter().collect();
        assert_eq!(best_order(&refs, None), best_order(&refs, None));
    }
}
