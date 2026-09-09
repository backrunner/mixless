use mixless_protocol::TrackId;

#[derive(Clone)]
pub(super) struct TrackOrder {
    tracks: Vec<TrackId>,
    position: Option<usize>,
    bag: Vec<usize>,
    shuffle: bool,
    random: u64,
}
impl TrackOrder {
    pub fn new(tracks: Vec<TrackId>, current: Option<TrackId>, seed: u64) -> Self {
        let position = tracks.iter().position(|id| Some(*id) == current);
        Self { tracks, position, bag: vec![], shuffle: false, random: seed.max(1) }
    }
    pub fn next(&mut self, shuffle: bool) -> TrackId {
        if shuffle != self.shuffle { self.bag.clear(); self.shuffle = shuffle; }
        let index = if !shuffle {
            self.position.map_or(0, |i| (i + 1) % self.tracks.len())
        } else {
            if self.bag.is_empty() {
                self.bag.extend(0..self.tracks.len());
                for i in (1..self.bag.len()).rev() {
                    self.random ^= self.random << 13;
                    self.random ^= self.random >> 7;
                    self.random ^= self.random << 17;
                    let j = self.random as usize % (i + 1);
                    self.bag.swap(i, j);
                }
                // A cycle boundary never repeats the just-played song when a
                // distinct song exists. All entries still play once per cycle.
                if let Some(previous) = self.position {
                    let last = self.bag.len() - 1;
                    if self.tracks[self.bag[last]] == self.tracks[previous] {
                        if let Some(other) = self.bag.iter().position(|i| self.tracks[*i] != self.tracks[previous]) {
                            self.bag.swap(other, last);
                        }
                    }
                }
            }
            self.bag.pop().unwrap()
        };
        self.position = Some(index);
        self.tracks[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequential_wraps_forever_and_keeps_duplicate_entries() {
        let mut order = TrackOrder::new(vec![TrackId(1),TrackId(2),TrackId(2)],Some(TrackId(1)),1);
        let actual: Vec<_> = (0..9).map(|_| order.next(false).0).collect();
        assert_eq!(actual, [2,2,1,2,2,1,2,2,1]);
    }
    #[test]
    fn shuffle_visits_every_track_each_cycle_without_boundary_repeats() {
        let mut order = TrackOrder::new((1..=6).map(TrackId).collect(),None,123);
        let mut previous = None;
        for _ in 0..20 {
            let cycle: Vec<_> = (0..6).map(|_| order.next(true)).collect();
            assert_ne!(cycle.first(), previous.as_ref());
            let mut sorted = cycle.clone(); sorted.sort_by_key(|id| id.0);
            assert_eq!(sorted, (1..=6).map(TrackId).collect::<Vec<_>>());
            previous = cycle.last().copied();
        }
    }
    #[test]
    fn one_track_repeats_and_mode_changes_continue_from_current_track() {
        let mut one = TrackOrder::new(vec![TrackId(9)],None,1);
        for _ in 0..10 { assert_eq!(one.next(true),TrackId(9)); }
        let mut many = TrackOrder::new((1..=4).map(TrackId).collect(),None,12);
        let current = many.next(true).0;
        assert_eq!(many.next(false).0, current % 4 + 1);
    }
}
