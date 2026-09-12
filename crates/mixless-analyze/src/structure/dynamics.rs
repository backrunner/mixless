//! Sustained bass/energy state complements timbral novelty. A dense, limited
//! DnB mix can keep the same timbre through a break; kick salience alone also
//! saturates on bass synths, so require level and attack evidence together.
use super::{quantile, BarFeature};

pub(super) fn driving(bars: &[BarFeature], level: f32, attacks: f32) -> Vec<bool> {
    let bass = quantile(bars.iter().map(|b| b.low_db), 0.8);
    let mut on = false;
    let mut state: Vec<_> = bars
        .iter()
        .map(|b| {
            let strong = b.rms >= level * 0.80
                && b.low_db >= bass - 3.5
                && b.kick_salience >= 0.45
                && b.onset_density >= attacks * 0.30;
            let sustained = b.rms >= level * 0.58
                && b.low_db >= bass - 6.
                && b.kick_salience >= 0.3
                && b.onset_density >= attacks * 0.15;
            on = strong || (on && sustained);
            on
        })
        .collect();
    // A one-bar fill or bass mute inside the drop is still part of the drop.
    let original = state.clone();
    for i in 1..state.len().saturating_sub(1) {
        if original[i - 1] && original[i + 1] {
            state[i] = true;
        }
    }
    // Brief two-bar variations cannot establish a new breakdown either.
    let mut i = 0;
    while i < state.len() {
        let start = i;
        let value = state[i];
        while i < state.len() && state[i] == value {
            i += 1;
        }
        if !value
            && start > 0
            && i < state.len()
            && bars[i - 1].end_sec - bars[start].start_sec < 6.
        {
            state[start..i].fill(true);
        }
    }
    state
}

pub(super) fn build_edges(bars: &[BarFeature], driving: &[bool]) -> Vec<usize> {
    let mut result = Vec::new();
    let mut i = 0;
    while i < bars.len() {
        let start = i;
        let state = driving[i];
        while i < bars.len() && driving[i] == state {
            i += 1;
        }
        if state || i - start < 8 || i == bars.len() {
            continue;
        }
        let base = quantile(bars[start..start + 3].iter().map(|b| b.rms), 0.5);
        let attacks = quantile(bars[start..start + 3].iter().map(|b| b.onset_density), 0.5);
        if let Some(at) = (start + 4..i - 2).find(|&at| {
            let tail = &bars[at..i];
            if bars[at..at + 2]
                .iter()
                .any(|b| b.rms <= base * 1.25 || b.onset_density <= attacks.max(0.1) * 1.5)
            {
                return false;
            }
            quantile(tail.iter().map(|b| b.rms), 0.25) > base * 1.25
                && quantile(tail.iter().map(|b| b.onset_density), 0.25) > attacks.max(0.1) * 1.5
        }) {
            result.push(at);
        }
    }
    result
}

pub(super) fn edges(state: &[bool]) -> Vec<usize> {
    (2..state.len().saturating_sub(2))
        .filter(|&i| {
            state[i - 2] == state[i - 1] && state[i] == state[i + 1] && state[i] != state[i - 1]
        })
        .collect()
}
