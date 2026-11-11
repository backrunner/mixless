//! Source-time queries recur across candidate lengths and part arrangements.
//! Keep exact float keys only for the lifetime of immutable analysis borrows.
use super::{Continuation, TrackAnalysis};
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

struct Cache {
    tracks: [usize; 2],
    values: HashMap<(usize, u32), Continuation>,
}
thread_local! { static CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) }; }

pub(crate) struct Scope<'a> {
    previous: Option<Cache>,
    borrowed: PhantomData<(&'a TrackAnalysis, Rc<()>)>,
}
impl<'a> Scope<'a> {
    pub(crate) fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        Self {
            previous: CACHE.with(|c| {
                c.replace(Some(Cache {
                    tracks: [a as *const _ as usize, b as *const _ as usize],
                    values: HashMap::new(),
                }))
            }),
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
    let key = (t as *const _ as usize, sec.to_bits());
    let enabled = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .is_some_and(|c| c.tracks.contains(&key.0))
    });
    if !enabled {
        return compute();
    }
    if let Some(value) = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|c| c.values.get(&key).copied())
    }) {
        return value;
    }
    let value = compute();
    CACHE.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut() {
            if c.values.len() >= 32768 {
                c.values.clear();
            }
            c.values.insert(key, value);
        }
    });
    value
}

#[cfg(test)]
mod tests {
    use super::*;
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
