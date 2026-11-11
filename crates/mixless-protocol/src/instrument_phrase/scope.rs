use super::{InstrumentBoundary, TrackAnalysis};
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::Arc};
type Cache = [(usize, Vec<InstrumentBoundary>); 2];
thread_local! { static CACHE: RefCell<Option<Arc<Cache>>> = const { RefCell::new(None) }; }

/// Optional call-scoped memoization for analysis consumers, tied to immutable inputs.
/// Nested scopes restore their parent; the scope cannot move across threads.
pub struct InstrumentScope<'a> {
    previous: Option<Arc<Cache>>,
    borrowed: PhantomData<(&'a TrackAnalysis, Rc<()>)>,
}
/// Share the measured boundaries, never a thread-local guard, across workers.
pub struct InstrumentEvidence<'a> {
    values: Arc<Cache>,
    borrowed: PhantomData<&'a TrackAnalysis>,
}
impl<'a> InstrumentEvidence<'a> {
    pub fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        Self {
            values: Arc::new(
                [a, b].map(|t| (t as *const _ as usize, super::instrument_boundaries(t))),
            ),
            borrowed: PhantomData,
        }
    }
    pub fn enter(&self) -> InstrumentScope<'a> {
        InstrumentScope {
            previous: CACHE.with(|c| c.replace(Some(Arc::clone(&self.values)))),
            borrowed: PhantomData,
        }
    }
}
impl<'a> InstrumentScope<'a> {
    pub fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        InstrumentEvidence::new(a, b).enter()
    }
}
impl Drop for InstrumentScope<'_> {
    fn drop(&mut self) {
        CACHE.with(|c| c.replace(self.previous.take()));
    }
}
pub(super) fn cached(t: &TrackAnalysis) -> Option<Vec<InstrumentBoundary>> {
    CACHE.with(|c| {
        c.borrow().as_ref().and_then(|v| {
            v.iter()
                .find(|(key, _)| *key == t as *const _ as usize)
                .map(|(_, v)| v.clone())
        })
    })
}
