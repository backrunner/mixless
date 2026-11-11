//! Prepare immutable whole-track evidence once for a group of worker jobs.
use crate::PlanContext;
use mixless_protocol::{InstrumentEvidence, PeakEvidence, TrackAnalysis};

pub(crate) struct Prepared<'a> {
    tracks: [&'a TrackAnalysis; 2],
    power: crate::level::evidence::Evidence<'a>,
    audible: crate::experience::Evidence<'a>,
    peaks: PeakEvidence<'a>,
    instruments: Option<InstrumentEvidence<'a>>,
}
impl<'a> Prepared<'a> {
    pub(crate) fn new(ctx: &PlanContext<'a>, stems: bool) -> Self {
        let (a, b) = (ctx.outgoing, ctx.incoming);
        Self {
            tracks: [a, b],
            power: crate::level::evidence::Evidence::new(a, b),
            audible: crate::experience::Evidence::new(a, b),
            peaks: PeakEvidence::new(a, b),
            instruments: stems.then(|| InstrumentEvidence::new(a, b)),
        }
    }
    pub(crate) fn run<T>(&self, f: impl FnOnce() -> T) -> T {
        let _power = self.power.enter();
        let _audible = self.audible.enter();
        let _peaks = self.peaks.enter();
        let _instruments = self.instruments.as_ref().map(InstrumentEvidence::enter);
        let [a, b] = self.tracks;
        let _patterns = crate::vocals::pattern::Scope::new(a, b);
        let _continuation = crate::vocals::Scope::new(a, b);
        f()
    }
}
