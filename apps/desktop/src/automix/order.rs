use super::QueueEntry;
use mixless_protocol::TrackId;

#[derive(Clone)]
pub(super) struct TrackOrder {
    tracks: Vec<TrackId>,
    position: Option<usize>,
    last: Option<QueueEntry>,
    previous: Option<QueueEntry>,
    bag: Vec<usize>,
    cycle: Vec<usize>,
    shuffle: bool,
    random: u64,
    deferred: std::collections::VecDeque<QueueEntry>,
}
impl TrackOrder {
    pub fn new(tracks: Vec<TrackId>, current: Option<TrackId>, seed: u64) -> Self {
        let position = tracks.iter().position(|id| Some(*id) == current);
        let last = position.map(|i| QueueEntry {
            track: tracks[i],
            occurrence: 0,
        });
        Self {
            tracks,
            position,
            last,
            previous: None,
            bag: vec![],
            cycle: vec![],
            shuffle: false,
            random: seed.max(1),
            deferred: Default::default(),
        }
    }
    pub fn replace(&mut self, tracks: Vec<TrackId>) {
        if tracks.is_empty() || tracks == self.tracks {
            return;
        }
        let current = self.current_entry();
        self.position = super::preparation::entries(&tracks)
            .iter()
            .position(|id| Some(*id) == current);
        self.tracks = tracks;
        let entries = super::preparation::entries(&self.tracks);
        self.deferred.retain(|entry| entries.contains(entry));
        self.bag.clear();
        self.cycle.clear();
    }

    pub fn rebase(&mut self, tracks: Vec<TrackId>, current: TrackId) {
        let occurrence = [self.last, self.previous]
            .into_iter()
            .flatten()
            .find(|e| e.track == current);
        self.replace(tracks);
        let entries = super::preparation::entries(&self.tracks);
        self.position = entries
            .iter()
            .position(|e| Some(*e) == occurrence)
            .or_else(|| self.tracks.iter().position(|id| *id == current));
        self.last = self.position.map(|i| entries[i]);
        self.deferred.clear();
    }

    pub fn matches(&self, tracks: &[TrackId]) -> bool {
        self.tracks == tracks
    }
    pub fn current_entry(&self) -> Option<QueueEntry> {
        self.last
    }

    /// Snapshot the actual random bag without drawing again in the UI.
    pub fn queue_rows(&self, outgoing: QueueEntry, shuffle: bool) -> Vec<QueueEntry> {
        let mut rows = vec![outgoing];
        if let Some(incoming) = self.current_entry() {
            if !rows.contains(&incoming) {
                rows.push(incoming);
            }
        }
        let mut copy = self.clone();
        for _ in 0..self.tracks.len() {
            copy.next(shuffle);
            if let Some(entry) = copy.current_entry() {
                if !rows.contains(&entry) {
                    rows.push(entry);
                }
            }
        }
        rows
    }
    pub fn has_deferred(&self) -> bool {
        !self.deferred.is_empty()
    }
    pub fn shuffled(&self) -> bool {
        self.shuffle
    }

    pub fn defer(&mut self, entry: QueueEntry) {
        if !self.deferred.contains(&entry) && self.current_entry() != Some(entry) {
            self.deferred.push_front(entry);
        }
    }

    pub fn preview(&self, count: usize, shuffle: bool) -> Vec<TrackId> {
        self.preview_entries(count, shuffle)
            .into_iter()
            .map(|e| e.track)
            .collect()
    }

    pub fn preview_entries(&self, count: usize, shuffle: bool) -> Vec<QueueEntry> {
        let mut copy = self.clone();
        (0..count)
            .map(|_| {
                copy.next(shuffle);
                copy.current_entry().unwrap()
            })
            .collect()
    }

    pub fn next(&mut self, shuffle: bool) -> TrackId {
        if let Some(entry) = self.deferred.pop_front() {
            let entries = super::preparation::entries(&self.tracks);
            self.bag.retain(|i| entries[*i] != entry);
            if self
                .position
                .is_none_or(|i| entries[(i + 1) % self.tracks.len()] == entry)
            {
                self.position = entries.iter().position(|t| *t == entry);
            }
            self.previous = self.last;
            self.last = Some(entry);
            return entry.track;
        }
        if shuffle != self.shuffle {
            self.bag.clear();
            self.cycle.clear();
            self.shuffle = shuffle;
        }
        let index = if !shuffle {
            self.position.map_or(0, |i| (i + 1) % self.tracks.len())
        } else {
            if self.bag.is_empty() {
                if !self.cycle.is_empty() {
                    self.bag = self.cycle.clone();
                } else {
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
                            if let Some(other) = self
                                .bag
                                .iter()
                                .position(|i| self.tracks[*i] != self.tracks[previous])
                            {
                                self.bag.swap(other, last);
                            }
                        }
                        // The currently sounding occurrence anchors this circuit;
                        // put it after every other upcoming occurrence at wrap.
                        let at = self.bag.iter().position(|i| *i == previous).unwrap();
                        self.bag.swap(at, 0);
                    }
                    // A Shuffle selection owns one stable circuit. Advancing the
                    // render clock cannot redraw future rows or their forecasts.
                    self.cycle = self.bag.clone();
                }
            }
            self.bag.pop().unwrap()
        };
        self.position = Some(index);
        self.previous = self.last;
        self.last = Some(QueueEntry {
            track: self.tracks[index],
            occurrence: self.tracks[..index]
                .iter()
                .filter(|id| **id == self.tracks[index])
                .count(),
        });
        self.tracks[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sequential_wraps_forever_and_keeps_duplicate_entries() {
        let mut order = TrackOrder::new(
            vec![TrackId(1), TrackId(2), TrackId(2)],
            Some(TrackId(1)),
            1,
        );
        let actual: Vec<_> = (0..9).map(|_| order.next(false).0).collect();
        assert_eq!(actual, [2, 2, 1, 2, 2, 1, 2, 2, 1]);
    }
    #[test]
    fn shuffle_visits_every_track_each_cycle_without_boundary_repeats() {
        let mut order = TrackOrder::new((1..=6).map(TrackId).collect(), None, 123);
        let mut previous = None;
        for _ in 0..20 {
            let cycle: Vec<_> = (0..6).map(|_| order.next(true)).collect();
            assert_ne!(cycle.first(), previous.as_ref());
            let mut sorted = cycle.clone();
            sorted.sort_by_key(|id| id.0);
            assert_eq!(sorted, (1..=6).map(TrackId).collect::<Vec<_>>());
            previous = cycle.last().copied();
        }
    }
    #[test]
    fn one_track_repeats_and_mode_changes_continue_from_current_track() {
        let mut one = TrackOrder::new(vec![TrackId(9)], None, 1);
        for _ in 0..10 {
            assert_eq!(one.next(true), TrackId(9));
        }
        let mut many = TrackOrder::new((1..=4).map(TrackId).collect(), None, 12);
        let current = many.next(true).0;
        assert_eq!(many.next(false).0, current % 4 + 1);
    }

    #[test]
    fn published_shuffle_circuit_matches_every_executed_occurrence_without_redrawing() {
        for seed in 1..16 {
            let tracks = vec![TrackId(1), TrackId(2), TrackId(1), TrackId(3), TrackId(4)];
            let mut order = TrackOrder::new(tracks, Some(TrackId(1)), seed);
            let outgoing = order.current_entry().unwrap();
            order.next(true);
            let rows = order.queue_rows(outgoing, true);
            assert_eq!(rows.len(), 5);
            assert_eq!(rows[1], order.current_entry().unwrap());
            assert_eq!(rows, order.queue_rows(outgoing, true));
            for i in 2..17 {
                order.next(true);
                assert_eq!(order.current_entry(), Some(rows[i % rows.len()]));
            }
        }
    }

    #[test]
    fn deferred_repeated_recording_keeps_its_source_occurrence() {
        let mut order = TrackOrder::new(
            vec![TrackId(1), TrackId(2), TrackId(1), TrackId(3)],
            Some(TrackId(1)),
            1,
        );
        let later = QueueEntry {
            track: TrackId(1),
            occurrence: 1,
        };
        order.defer(later);
        assert_eq!(order.next(false), TrackId(1));
        assert_eq!(order.current_entry(), Some(later));
        assert_eq!(order.next(false), TrackId(2));
    }
}

#[cfg(test)]
mod updates {
    use super::*;
    #[test]
    fn appended_files_join_the_running_playlist_without_restarting_it() {
        let mut order = TrackOrder::new(vec![TrackId(1), TrackId(2)], Some(TrackId(1)), 1);
        assert_eq!(order.next(false), TrackId(2));
        order.replace(vec![TrackId(1), TrackId(2), TrackId(3)]);
        assert_eq!(order.next(false), TrackId(3));
        assert_eq!(order.next(false), TrackId(1));
    }
}
