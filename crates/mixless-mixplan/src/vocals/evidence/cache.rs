//! Source-time queries recur across candidate lengths and part arrangements.
//! Keep exact float keys only for the lifetime of immutable analysis borrows.
use super::{Continuation, TrackAnalysis};
use crate::evidence::cache::{Local, Shared};
use std::{cell::RefCell, marker::PhantomData, rc::Rc, sync::Arc};
type Cache = Local<Continuation>;

pub(crate) fn shared(a: &TrackAnalysis, b: &TrackAnalysis) -> Option<Arc<Shared<Continuation>>> {
    CACHE.with(|c| c.borrow().as_ref().and_then(|c| c.shared(a, b)))
}
thread_local! { static CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) }; }

pub(crate) struct Scope<'a> {
    previous: Option<Cache>,
    borrowed: PhantomData<(&'a TrackAnalysis, Rc<()>)>,
}
impl<'a> Scope<'a> {
    #[cfg(test)]
    pub(crate) fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        Self::with_cache(a, b, Arc::default())
    }
    pub(crate) fn with_cache(
        a: &'a TrackAnalysis,
        b: &'a TrackAnalysis,
        shared: Arc<Shared<Continuation>>,
    ) -> Self {
        Self {
            previous: CACHE.with(|c| c.replace(Some(Cache::new(a, b, shared)))),
            borrowed: PhantomData,
        }
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        CACHE.with(|c| c.replace(self.previous.take()));
    }
}

pub(super) fn continuation(
    t: &TrackAnalysis,
    sec: f32,
    compute: impl FnOnce() -> Continuation,
) -> Continuation {
    let key = (t as *const _ as usize, sec.to_bits() as usize);
    if let Some(value) = CACHE.with(|c| c.borrow_mut().as_mut().and_then(|c| c.get(key))) {
        return value;
    }
    let value = compute();
    CACHE.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            c.insert(key, value);
        }
    });
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[cfg(not(feature = "trace"))]
    fn request_evidence_crosses_workers_pairs_and_repeated_preparations() {
        let mut a = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            32,
            0.8,
            0.2,
        );
        let b = a.clone();
        let c = a.clone(); // Identical IDs/payloads still have distinct identities.
        fn context<'a>(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> crate::PlanContext<'a> {
            crate::PlanContext {
                outgoing: a,
                incoming: b,
                cues_out: &[],
                cues_in: &[],
                offset_a: Default::default(),
                offset_b: Default::default(),
            }
        }
        {
            let session = crate::memo::Session::new(&[&a, &b, &c]);
            let _scope = session.enter();
            crate::evidence::Prepared::new(&context(&a, &b), true).run(|| {
                assert_eq!(
                    continuation(&a, 1., || Continuation::Continuing),
                    Continuation::Continuing
                );
            });
            let evidence = crate::evidence::Prepared::new(&context(&a, &c), true);
            let values = crate::order::parallel::map(40, |_| {
                evidence.run(|| {
                    assert_eq!(
                        continuation(&c, 1., || Continuation::Uncertain),
                        Continuation::Uncertain
                    );
                    continuation(&a, 1., || panic!("request evidence lost on a worker"))
                })
            });
            assert_eq!(values, vec![Continuation::Continuing; 40]);
        }
        a.duration_sec = 12.;
        let session = crate::memo::Session::new(&[&a, &b]);
        let _scope = session.enter();
        crate::evidence::Prepared::new(&context(&a, &b), true).run(|| {
            assert_eq!(
                continuation(&a, 1., || Continuation::Released),
                Continuation::Released
            );
        });
    }

    #[test]
    fn exact_times_nested_scopes_and_unborrowed_tracks_are_isolated() {
        let mut a = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            32,
            0.8,
            0.2,
        );
        let b = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            32,
            0.8,
            0.2,
        );
        let other = crate::tests::track(
            1,
            120.,
            "8A",
            mixless_protocol::SectionLabel::Outro,
            32,
            0.8,
            0.2,
        );
        {
            let _scope = Scope::new(&a, &b);
            assert_eq!(
                continuation(&a, 1., || Continuation::Continuing),
                Continuation::Continuing
            );
            assert_eq!(
                continuation(&a, 1., || panic!("cache miss")),
                Continuation::Continuing
            );
            assert_eq!(
                continuation(&a, f32::from_bits(1f32.to_bits() + 1), || {
                    Continuation::Released
                }),
                Continuation::Released
            );
            assert_eq!(
                continuation(&b, 1., || Continuation::Released),
                Continuation::Released
            );
            assert_eq!(
                continuation(&other, 1., || Continuation::Uncertain),
                Continuation::Uncertain
            );
            assert_eq!(
                continuation(&other, 1., || Continuation::Released),
                Continuation::Released
            );
            {
                let _nested = Scope::new(&a, &other);
                assert_eq!(
                    continuation(&a, 1., || Continuation::Released),
                    Continuation::Released
                );
            }
            assert_eq!(
                continuation(&a, 1., || panic!("parent lost")),
                Continuation::Continuing
            );
        }
        a.duration_sec = 20.;
        let _scope = Scope::new(&a, &b);
        assert_eq!(
            continuation(&a, 1., || Continuation::Released),
            Continuation::Released
        );
    }
}
