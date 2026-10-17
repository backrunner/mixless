//! Playlist order for the smoothest set: one loop through every track, chosen
//! by planning the real transition between each pair and keeping the cycle
//! whose transitions score best. Pure and deterministic; computed once per
//! playlist, then played round and round in that order.
/// Increment when ordering or the transition planner changes cached Best results.
pub const ORDER_VERSION: u32 = 3;

use crate::{Planner, PlannerOptions};
use mixless_protocol::TrackAnalysis;

/// Rough cost of mixing `a` into `b` from tempo, key and level alone; 0 is
/// an ideal pair, about 1 needs a native-rate handoff. It breaks ties
/// between planned transitions and picks which pairs are worth planning.
pub fn transition_cost(a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    tempo_cost(a, b)
        + key_cost(a, b)
        + energy_cost(a, b)
        + if same_recording(a, b) { 3. } else { 0. }
}

/// Two playlist entries of one recording (a duplicate import) mix
/// "perfectly" into each other, which would play the song twice in a row.
fn same_recording(a: &TrackAnalysis, b: &TrackAnalysis) -> bool {
    (a.duration_sec - b.duration_sec).abs() < 1.
        && (a.tempo.global_bpm - b.tempo.global_bpm).abs() < 0.5
        && a.camelot == b.camelot
        && a.bars.len().abs_diff(b.bars.len()) <= 1
}

/// Cost of the transition the planner would actually make from `a` into
/// `b`, entering `a` at its start: the inverse of its score, so a long
/// safe blend beats an emergency handoff.
fn planned_cost(planner: &Planner, a: &TrackAnalysis, b: &TrackAnalysis) -> f32 {
    let plan = planner.plan_pair(a, b, &[], &[], Default::default(), Default::default());
    let rough = 0.15 * transition_cost(a, b);
    match &plan.summary {
        Some(summary) if plan.failure_reason.is_none() && !same_recording(a, b) => {
            // Reaching A's last stretch with no real overlap plays it out:
            // a pair that can only do that is a worse neighbour than its
            // score alone says.
            let played_out = plan.t_out_a >= (a.duration_sec * 0.92).min(a.duration_sec - 8.)
                && summary.length_bars < 4;
            (1. - summary.score).max(0.) + rough + if played_out { 0.15 } else { 0. }
        }
        _ => 1.5 + rough,
    }
}

/// Each track keeps this many of its roughly closest successors in a large
/// playlist; every pair is planned in a small one.
const PLANNED_SUCCESSORS: usize = 12;
/// Beyond this many tracks a full library is ordered on the rough cost
/// alone; planning thousands of pairs would keep AUTO waiting.
const PLANNED_TRACKS: usize = 200;

/// Every index once, as a loop: playing on from the last entry returns to
/// the first. The loop is opened after its weakest transition.
pub fn best_cycle(tracks: &[&TrackAnalysis], options: &PlannerOptions) -> Vec<usize> {
    best_cycle_with_progress(tracks, options, |_, _| {})
}

/// Reports completed work units, not elapsed-time estimates. Pair evaluations
/// run in parallel, so observers must accept reports arriving out of order.
pub fn best_cycle_with_progress(
    tracks: &[&TrackAnalysis],
    options: &PlannerOptions,
    progress: impl Fn(usize, usize) + Sync,
) -> Vec<usize> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let n = tracks.len();
    if n < 3 {
        progress(1, 1);
        return (0..n).collect();
    }
    let successors = if n <= 40 { n - 1 } else { PLANNED_SUCCESSORS };
    let pair_count = if n > PLANNED_TRACKS {
        0
    } else {
        n * successors
    };
    let total = n + pair_count + order_starts(n) + 1;
    let done = AtomicUsize::new(0);
    let advance = || progress(done.fetch_add(1, Ordering::Relaxed) + 1, total);
    progress(0, total);
    let rough: Vec<Vec<f32>> = tracks
        .iter()
        .map(|a| {
            let row = tracks.iter().map(|b| transition_cost(a, b)).collect();
            advance();
            row
        })
        .collect();
    if n > PLANNED_TRACKS {
        let order = order_by_with_progress(&rough, &advance);
        advance();
        return order;
    }
    let mut pairs = Vec::new();
    for (i, row) in rough.iter().enumerate() {
        let mut next: Vec<usize> = (0..n).filter(|j| *j != i).collect();
        next.sort_by(|x, y| row[*x].total_cmp(&row[*y]).then(x.cmp(y)));
        pairs.extend(next.into_iter().take(successors).map(|j| (i, j)));
    }
    let planner = Planner::with_options(PlannerOptions {
        earliest_outgoing_sec: 0.,
        outgoing_entry_sec: 0.,
        strategy: None,
        ..options.clone()
    });
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(pairs.len());
    let costs: Vec<f32> = std::thread::scope(|scope| {
        let chunks: Vec<_> = pairs
            .chunks(pairs.len().div_ceil(threads.max(1)))
            .map(|chunk| {
                let planner = &planner;
                let advance = &advance;
                scope.spawn(move || {
                    chunk
                        .iter()
                        .map(|&(i, j)| {
                            let cost = planned_cost(planner, tracks[i], tracks[j]);
                            advance();
                            cost
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        chunks
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let mut cost: Vec<Vec<f32>> = rough
        .iter()
        .map(|row| row.iter().map(|r| 2. + r).collect())
        .collect();
    for (&(i, j), c) in pairs.iter().zip(costs) {
        cost[i][j] = c;
    }
    let order = order_by_with_progress(&cost, &advance);
    advance();
    order
}

fn order_starts(n: usize) -> usize {
    if n > PLANNED_TRACKS { 1 } else { n.min(16) }
}

/// The cheapest loop under `cost[from][to]`, opened after its most
/// expensive edge so a listener starting at the top hears every good
/// transition before the one forced by closing the loop.
#[cfg(test)]
fn order_by(cost: &[Vec<f32>]) -> Vec<usize> {
    order_by_with_progress(cost, &|| {})
}

fn order_by_with_progress(cost: &[Vec<f32>], advance: &impl Fn()) -> Vec<usize> {
    let n = cost.len();
    let total = |tour: &[usize]| {
        (0..n)
            .map(|k| cost[tour[k]][tour[(k + 1) % n]])
            .sum::<f32>()
    };
    let mut best: Option<(f32, Vec<usize>)> = None;
    // A large playlist tries a spread of starts instead of every one.
    let starts = order_starts(n);
    for start in (0..starts).map(|k| k * n / starts) {
        let mut tour = nearest_neighbour(cost, start);
        improve(cost, &mut tour);
        let length = total(&tour);
        if best.as_ref().is_none_or(|(b, _)| length < *b - 1e-6) {
            best = Some((length, tour));
        }
        advance();
    }
    let mut tour = best.map(|(_, t)| t).unwrap_or_default();
    let worst = (0..n)
        .max_by(|x, y| {
            cost[tour[*x]][tour[(*x + 1) % n]]
                .total_cmp(&cost[tour[*y]][tour[(*y + 1) % n]])
                .then(y.cmp(x))
        })
        .unwrap_or(n - 1);
    tour.rotate_left((worst + 1) % n);
    tour
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

/// Or-opt on the loop: move runs of one to three tracks to a better place,
/// keeping their direction because a transition is not symmetric.
fn improve(cost: &[Vec<f32>], tour: &mut Vec<usize>) {
    let n = tour.len();
    // Each pass is quadratic; a long list settles for fewer passes.
    for _ in 0..(64 * 100 / n.max(100)).max(2) {
        let mut changed = false;
        for run in 1..=3.min(n - 2) {
            for from in 0..n {
                // Cut the run out of the loop, then try every other gap.
                let at = |k: usize| tour[k % n];
                let (before, first, last, after) = (
                    at(from + n - 1),
                    at(from),
                    at(from + run - 1),
                    at(from + run),
                );
                let saved = cost[before][first] + cost[last][after] - cost[before][after];
                let mut best = (1e-6f32, None);
                for gap in 0..n - run {
                    let (u, v) = (at(from + run + gap), at(from + run + gap + 1));
                    if v == first {
                        continue;
                    }
                    let gain = saved - (cost[u][first] + cost[last][v] - cost[u][v]);
                    if gain > best.0 {
                        best = (gain, Some(gap));
                    }
                }
                if let Some(gap) = best.1 {
                    let mut rest: Vec<usize> = (0..n).map(|k| at(from + k)).collect();
                    let moved: Vec<usize> = rest.drain(..run).collect();
                    rest.splice(gap + 1..gap + 1, moved);
                    *tour = rest;
                    changed = true;
                }
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

    fn rough(tracks: &[TrackAnalysis]) -> Vec<Vec<f32>> {
        tracks
            .iter()
            .map(|a| tracks.iter().map(|b| transition_cost(a, b)).collect())
            .collect()
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
        let order = order_by(&rough(&tracks));
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..6).collect::<Vec<_>>());
        // The loop crosses between the islands twice; opened after the
        // worse crossing, the playlist reads with a single jump.
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
    fn the_planned_loop_is_fixed_and_visits_every_track_once() {
        let tracks: Vec<_> = [128., 140., 126., 174., 130., 172.]
            .iter()
            .enumerate()
            .map(|(i, bpm)| track(i as i64, *bpm, "5A"))
            .collect();
        let refs: Vec<_> = tracks.iter().collect();
        let options = PlannerOptions::default();
        let order = best_cycle(&refs, &options);
        let reports = std::sync::Mutex::new(Vec::new());
        assert_eq!(
            order,
            best_cycle_with_progress(&refs, &options, |done, total| {
                reports.lock().unwrap().push((done, total));
            })
        );
        let mut reports = reports.into_inner().unwrap();
        let total = reports[0].1;
        assert_eq!(reports.first(), Some(&(0, total)));
        assert_eq!(reports.last(), Some(&(total, total)));
        assert!(
            reports
                .iter()
                .all(|&(done, count)| done <= count && count == total)
        );
        reports.sort_unstable();
        assert_eq!(
            reports,
            (0..=total).map(|done| (done, total)).collect::<Vec<_>>()
        );
        let mut sorted = order.clone();
        sorted.sort();
        assert_eq!(sorted, (0..6).collect::<Vec<_>>());
    }

    #[test]
    fn progress_finishes_for_short_and_rough_only_playlists() {
        for count in [0, 1, 2, PLANNED_TRACKS + 1] {
            let tracks: Vec<_> = (0..count).map(|i| track(i as i64, 128., "5A")).collect();
            let refs: Vec<_> = tracks.iter().collect();
            let last = std::sync::Mutex::new((0, 0));
            let order =
                best_cycle_with_progress(&refs, &PlannerOptions::default(), |done, total| {
                    *last.lock().unwrap() = (done, total);
                });
            assert_eq!(order.len(), count);
            let (done, total) = last.into_inner().unwrap();
            assert!(total > 0);
            assert_eq!(done, total);
        }
    }

    #[test]
    fn a_duplicate_import_never_follows_itself() {
        let mut tracks: Vec<_> = [128., 128., 127., 129.]
            .iter()
            .enumerate()
            .map(|(i, bpm)| track(i as i64, *bpm, "5A"))
            .collect();
        tracks[1].duration_sec = tracks[0].duration_sec;
        let order = order_by(&rough(&tracks));
        let at = |i| order.iter().position(|x| *x == i).unwrap();
        assert_ne!((at(0) as i32 - at(1) as i32).rem_euclid(4), 1);
        assert_ne!((at(1) as i32 - at(0) as i32).rem_euclid(4), 1);
    }
}
