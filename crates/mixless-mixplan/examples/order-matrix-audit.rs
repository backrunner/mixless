//! Exact Held-Karp reference for a saved cold-pair cost matrix (at most 18
//! tracks). This cannot prove the optimum of stateful playback or listening.
//! Usage: order-matrix-audit MIXLESS_ORDER_AUDIT_DIRECTORY
use serde_json::{json, Value};
use std::path::Path;

fn exact(cost: &[Vec<f64>]) -> (f64, Vec<usize>) {
    let n = cost.len();
    let mut dp = vec![f64::INFINITY; (1 << n) * n];
    let mut parent = vec![usize::MAX; dp.len()];
    dp[n] = 0.; // mask = 1, last = 0
    for mask in (1..1 << n).step_by(2) {
        for last in 0..n {
            let prefix = dp[mask * n + last];
            if !prefix.is_finite() {
                continue;
            }
            for next in 1..n {
                if mask & (1 << next) != 0 {
                    continue;
                }
                let target = (mask | (1 << next)) * n + next;
                let value = prefix + cost[last][next];
                if value < dp[target] {
                    dp[target] = value;
                    parent[target] = last;
                }
            }
        }
    }
    let full = (1 << n) - 1;
    let mut last = (1..n)
        .min_by(|a, b| {
            (dp[full * n + *a] + cost[*a][0]).total_cmp(&(dp[full * n + *b] + cost[*b][0]))
        })
        .unwrap();
    let value = dp[full * n + last] + cost[last][0];
    let mut tour = vec![];
    let mut mask = full;
    loop {
        tour.push(last);
        if last == 0 {
            break;
        }
        let previous = parent[mask * n + last];
        mask ^= 1 << last;
        last = previous;
    }
    tour.reverse();
    (value, tour)
}

fn total(cost: &[Vec<f64>], tour: &[usize]) -> f64 {
    tour.iter()
        .zip(tour.iter().cycle().skip(1))
        .map(|(&a, &b)| cost[a][b])
        .sum()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let arg = std::env::args()
        .nth(1)
        .ok_or("usage: order-matrix-audit AUDIT_DIRECTORY")?;
    let dir = Path::new(&arg);
    let matrix: Value = serde_json::from_slice(&std::fs::read(dir.join("matrix.json"))?)?;
    let mut cost: Vec<Vec<f64>> = serde_json::from_value(matrix["cost"].clone())?;
    let n = cost.len();
    if !(2..=18).contains(&n)
        || cost
            .iter()
            .any(|r| r.len() != n || r.iter().any(|x| !x.is_finite()))
    {
        return Err("expected a finite square matrix of 2 to 18 tracks".into());
    }
    let mut output = vec![];
    for round in 0.. {
        let path = dir.join(format!("round-{round}.json"));
        if !path.exists() {
            break;
        }
        let record: Value = serde_json::from_slice(&std::fs::read(path)?)?;
        let tour: Vec<usize> = serde_json::from_value(record["order"].clone())?;
        let mut indices = tour.clone();
        indices.sort_unstable();
        if indices != (0..n).collect::<Vec<_>>() {
            return Err("invalid recorded tour".into());
        }
        let (optimal, exact_tour) = exact(&cost);
        let heuristic = total(&cost, &tour);
        output.push(json!({"round": round, "matrix_optimal": optimal,
            "matrix_heuristic": heuristic, "gap": heuristic - optimal,
            "exact_order": exact_tour, "observed_chain_cost": record["total"]}));
        for correction in record["corrections"]
            .as_array()
            .ok_or("missing corrections")?
        {
            let a = correction[0].as_u64().ok_or("invalid index")? as usize;
            let b = correction[1].as_u64().ok_or("invalid index")? as usize;
            if a >= n || b >= n {
                return Err("invalid correction index".into());
            }
            cost[a][b] = cost[a][b].max(correction[2].as_f64().ok_or("invalid cost")?);
        }
        for (field, penalty) in [("failed", 1.), ("rushed", 0.2)] {
            for pair in record[field].as_array().ok_or("missing penalty list")? {
                let a = pair[0].as_u64().ok_or("invalid index")? as usize;
                let b = pair[1].as_u64().ok_or("invalid index")? as usize;
                if a >= n || b >= n {
                    return Err("invalid penalty index".into());
                }
                // Production updates f32 matrix entries between rounds.
                cost[a][b] = (cost[a][b] as f32 + penalty as f32) as f64;
            }
        }
    }
    if output.is_empty() {
        let (optimal, tour) = exact(&cost);
        output.push(json!({"matrix_optimal": optimal, "exact_order": tour}));
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

#[test]
fn directed_cycle_reference_matches_every_permutation_on_small_matrices() {
    fn enumerate(cost: &[Vec<f64>], tour: &mut Vec<usize>, best: &mut f64) {
        if tour.len() == cost.len() {
            *best = best.min(total(cost, tour));
            return;
        }
        for i in 1..cost.len() {
            if !tour.contains(&i) {
                tour.push(i);
                enumerate(cost, tour, best);
                tour.pop();
            }
        }
    }
    for n in 2..=7 {
        let cost: Vec<Vec<f64>> = (0..n)
            .map(|i| {
                (0..n)
                    .map(|j| ((i * 31 + j * 17 + i * j * 7) % 23) as f64 / 8.)
                    .collect()
            })
            .collect();
        let mut best = f64::INFINITY;
        enumerate(&cost, &mut vec![0], &mut best);
        let (value, tour) = exact(&cost);
        assert_eq!(value, best);
        assert_eq!(total(&cost, &tour), best);
    }
}
