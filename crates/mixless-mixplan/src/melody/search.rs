//! Reuse phrase-window searches while fitting one immutable pair of clocks.
use crate::PlanContext;
use mixless_protocol::{MixPlan, Polyline, TrackAnalysis};
use std::collections::HashMap;

type Windows = Vec<(f32, f32)>;
pub(crate) struct Search<'a> {
    tracks: [&'a TrackAnalysis; 2],
    sources: [&'a Polyline; 2],
    n: f32,
    values: HashMap<(bool, bool, u32, u32), Windows>,
}
impl<'a> Search<'a> {
    pub(crate) fn new(ctx: &'a PlanContext<'_>, p: &'a MixPlan, n: f32) -> Self {
        Self {
            tracks: [ctx.outgoing, ctx.incoming],
            sources: [&p.outgoing_source, &p.incoming_source],
            n,
            values: HashMap::new(),
        }
    }

    pub(crate) fn windows(&mut self, outgoing: bool, desired: (f32, f32)) -> Windows {
        self.get(outgoing, desired, false)
    }

    pub(crate) fn note_windows(&mut self, outgoing: bool, desired: (f32, f32)) -> Windows {
        self.get(outgoing, desired, true)
    }

    fn get(&mut self, outgoing: bool, desired: (f32, f32), notes: bool) -> Windows {
        let key = (outgoing, notes, desired.0.to_bits(), desired.1.to_bits());
        if let Some(value) = self.values.get(&key) {
            return value.clone();
        }
        let d = usize::from(!outgoing);
        let compute = if notes {
            super::note_windows
        } else {
            super::windows
        };
        let value = compute(self.tracks[d], self.sources[d], self.n, desired, outgoing);
        if self.values.len() >= 1024 {
            self.values.clear();
        }
        self.values.insert(key, value.clone());
        value
    }
}
