//! Between transitions the outgoing deck is untouched; these moves add a
//! little played-in feel. Any hand on the lane ends the gesture for the rest
//! of the track; the transition plan always takes the lanes back.
use mixless_mixplan::{PerformanceLane, PerformanceMove};
use mixless_protocol::{Command, DeckId};
use std::collections::{HashMap, HashSet};

// A snapshot value this far from what we last sent is the user's hand, not
// automation drift: the lane is theirs for the rest of the track.
const FILTER_TOUCH: f32 = 0.02;
const STEM_TOUCH: f32 = 0.05;
// Below this, consecutive sends are inaudible churn on the command channel.
const SEND_EPSILON: f32 = 0.004;

fn neutral(lane: PerformanceLane) -> f32 {
    match lane {
        PerformanceLane::Filter => 0.,
        PerformanceLane::Stem(_) => 1.,
    }
}

fn command(deck: DeckId, lane: PerformanceLane, value: f32) -> Command {
    match lane {
        PerformanceLane::Filter => Command::SetChannelFilter {
            deck,
            amount: value,
        },
        PerformanceLane::Stem(stem) => Command::SetStemGain { deck, stem, value },
    }
}

pub(super) struct Performer {
    moves: Vec<PerformanceMove>,
    last_sent: HashMap<PerformanceLane, f32>,
    taken_over: HashSet<PerformanceLane>,
    active: HashSet<PerformanceLane>,
}

impl Performer {
    pub fn new(moves: Vec<PerformanceMove>) -> Self {
        Self {
            moves,
            last_sent: HashMap::new(),
            taken_over: HashSet::new(),
            active: HashSet::new(),
        }
    }

    /// Called each host tick with the outgoing deck's source seconds and the
    /// lanes' current snapshot values. Returns the commands to dispatch.
    pub fn tick(
        &mut self,
        seconds: f32,
        filter_amount: f32,
        stem_gain: [f32; 3],
        deck: DeckId,
    ) -> Vec<Command> {
        let mut out = Vec::new();
        let mut engaged = HashSet::new();
        for i in 0..self.moves.len() {
            let lane = self.moves[i].lane;
            let m = &self.moves[i];
            if seconds < m.start_sec || seconds > m.end_sec {
                continue;
            }
            engaged.insert(lane);
            if self.taken_over.contains(&lane) {
                continue;
            }
            let (snapshot, touch) = match lane {
                PerformanceLane::Filter => (filter_amount, FILTER_TOUCH),
                PerformanceLane::Stem(stem) => (stem_gain[stem.index()], STEM_TOUCH),
            };
            let value = m.curve.sample(seconds);
            match self.last_sent.get(&lane) {
                // We hold this lane: drift away from our command is the user.
                Some(&sent) if (snapshot - sent).abs() > touch => {
                    self.taken_over.insert(lane);
                    self.active.remove(&lane);
                    continue;
                }
                // First touch inside the window: a lane already off neutral
                // belongs to whoever set it there. (Every curve starts at
                // neutral, so neutral is the value the lane would hold
                // without us.)
                None if (snapshot - neutral(lane)).abs() > touch => {
                    self.taken_over.insert(lane);
                    continue;
                }
                _ => {}
            }
            if self
                .last_sent
                .get(&lane)
                .is_none_or(|&sent| (value - sent).abs() > SEND_EPSILON)
            {
                out.push(command(deck, lane, value));
                self.last_sent.insert(lane, value);
                self.active.insert(lane);
            }
        }
        // A finished move returns its lane to neutral exactly once.
        let ended: Vec<_> = self
            .active
            .iter()
            .copied()
            .filter(|lane| !engaged.contains(lane))
            .collect();
        for lane in ended {
            self.active.remove(&lane);
            out.push(command(deck, lane, neutral(lane)));
            self.last_sent.insert(lane, neutral(lane));
        }
        out
    }

    /// Neutral-restoring commands for lanes currently mid-move — used when the
    /// transition starts or AUTO stops. Lanes the user took are left alone.
    pub fn release(&mut self, deck: DeckId) -> Vec<Command> {
        let lanes: Vec<_> = self.active.drain().collect();
        lanes
            .into_iter()
            .map(|lane| {
                self.last_sent.insert(lane, neutral(lane));
                command(deck, lane, neutral(lane))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::Polyline;

    fn moves() -> Vec<PerformanceMove> {
        vec![
            PerformanceMove {
                lane: PerformanceLane::Filter,
                start_sec: 1.,
                end_sec: 3.,
                curve: Polyline {
                    nodes: vec![(1., 0.), (3., 0.5)],
                },
                label: "riser",
            },
            PerformanceMove {
                lane: PerformanceLane::Stem(mixless_protocol::StemKind::Drums),
                start_sec: 2.,
                end_sec: 2.5,
                curve: Polyline {
                    nodes: vec![(2., 1.), (2.2, 0.), (2.5, 1.)],
                },
                label: "pull",
            },
        ]
    }

    #[test]
    fn user_touch_on_one_lane_surrenders_only_that_lane() {
        let mut p = Performer::new(moves());
        // Inside both moves: user has grabbed the filter.
        let out = p.tick(1.2, 0.4, [1., 1., 1.], DeckId::A);
        assert!(
            out.is_empty()
                || out
                    .iter()
                    .all(|c| !matches!(c, Command::SetChannelFilter { .. }))
        );
        p.tick(1.4, 0.4, [1., 1., 1.], DeckId::A);
        let out = p.tick(2.2, 0.45, [1., 1., 1.], DeckId::A);
        // Filter lane surrendered: no filter commands ever, drums still move.
        assert!(
            out.iter()
                .all(|c| !matches!(c, Command::SetChannelFilter { .. }))
        );
        assert!(out.iter().any(|c| matches!(
            c,
            Command::SetStemGain {
                stem: mixless_protocol::StemKind::Drums,
                ..
            }
        )));
        // The surrendered lane is also not restored by release.
        let out = p.release(DeckId::A);
        assert!(
            out.iter()
                .all(|c| !matches!(c, Command::SetChannelFilter { .. }))
        );
    }

    #[test]
    fn a_finished_move_restores_neutral_once() {
        let mut p = Performer::new(moves());
        assert!(
            p.tick(1.5, 0., [1., 1., 1.], DeckId::A)
                .iter()
                .any(|c| matches!(
                    c,
                    Command::SetChannelFilter { amount, .. } if *amount > 0.1
                ))
        );
        // Inside the drum pull: the drums lane is now mid-move too. The
        // filter snapshot echoes our last send — no takeover.
        p.tick(2.2, 0.125, [1., 1., 1.], DeckId::A);
        // Past every move's end: each touched lane gets one neutral command.
        let out = p.tick(4., 0.3, [1., 0., 1.], DeckId::A);
        let restored: Vec<_> = out
            .iter()
            .filter(|c| match c {
                Command::SetChannelFilter { amount, .. } => *amount == 0.,
                Command::SetStemGain { value, .. } => *value == 1.,
                _ => false,
            })
            .collect();
        assert_eq!(restored.len(), 2, "{out:?}");
        // ...and never again.
        assert!(p.tick(4.5, 0., [1., 1., 1.], DeckId::A).is_empty());
    }

    #[test]
    fn release_restores_only_lanes_currently_moving() {
        let mut p = Performer::new(moves());
        // Before the drum pull's window only the filter is mid-move.
        p.tick(1.5, 0., [1., 1., 1.], DeckId::A);
        let out = p.release(DeckId::A);
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0],
            Command::SetChannelFilter { amount: 0., .. }
        ));
        assert!(p.release(DeckId::A).is_empty());
    }
}
