//! Convert immutable stem meters to linear power once, shared by worker scopes.
//! Keep the original frame selection and floating-point accumulation order.
use mixless_protocol::{StemFrame, TrackAnalysis};
use std::{
    cell::RefCell,
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, OnceLock},
};

type Power = [[f32; 3]; 3];
mod index;
struct TrackPower {
    index: index::Index,
    frames: Vec<Power>,
    reference: OnceLock<Option<f32>>,
}
type Values = [(usize, Arc<TrackPower>); 2];
thread_local! { static VALUES: RefCell<Option<Values>> = const { RefCell::new(None) }; }

pub(crate) struct Evidence<'a>(Values, PhantomData<&'a TrackAnalysis>);
pub(crate) struct Scope<'a>(Option<Values>, PhantomData<(&'a TrackAnalysis, Rc<()>)>);

fn measure(f: &StemFrame) -> Power {
    f.band_db.map(|bands| bands.map(|db| 10f32.powf(db / 10.)))
}

impl<'a> Evidence<'a> {
    pub(crate) fn new(a: &'a TrackAnalysis, b: &'a TrackAnalysis) -> Self {
        Self(
            [a, b].map(|t| {
                let key = t as *const _ as usize;
                let existing = VALUES.with(|v| {
                    v.borrow().as_ref().and_then(|v| {
                        v.iter()
                            .find(|(k, _)| *k == key)
                            .map(|(_, v)| Arc::clone(v))
                    })
                });
                (
                    key,
                    existing.unwrap_or_else(|| {
                        Arc::new(TrackPower {
                            index: index::Index::new(t.stems.as_ref().map_or(&[], |s| &s.frames)),
                            frames: t
                                .stems
                                .iter()
                                .flat_map(|s| &s.frames)
                                .map(measure)
                                .collect(),
                            reference: OnceLock::new(),
                        })
                    }),
                )
            }),
            PhantomData,
        )
    }

    pub(crate) fn enter(&self) -> Scope<'a> {
        Scope(
            VALUES.with(|v| v.replace(Some(self.0.clone()))),
            PhantomData,
        )
    }
}

impl Drop for Scope<'_> {
    fn drop(&mut self) {
        VALUES.with(|v| v.replace(self.0.take()));
    }
}

pub(crate) fn frame(t: &TrackAnalysis, sec: f32) -> Option<(&StemFrame, Power)> {
    let stems = t.stems.as_ref()?;
    with_track(t, |values| sample(&stems.frames, sec, values))
}

fn with_track<T>(t: &TrackAnalysis, f: impl FnOnce(Option<&TrackPower>) -> T) -> T {
    VALUES.with(|v| {
        let borrowed = v.borrow();
        f(borrowed.as_ref().and_then(|v| {
            v.iter()
                .find(|(key, _)| *key == t as *const _ as usize)
                .map(|(_, values)| values.as_ref())
        }))
    })
}

fn sample<'a>(
    frames: &'a [StemFrame],
    sec: f32,
    values: Option<&TrackPower>,
) -> Option<(&'a StemFrame, Power)> {
    let i = values.map_or_else(
        || frames.partition_point(|f| f.end_sec <= sec),
        |v| v.index.at(sec),
    );
    let f = frames.get(i).filter(|f| f.start_sec <= sec)?;
    Some((f, values.map_or_else(|| measure(f), |v| v.frames[i])))
}

/// Resolve the scoped evidence once for the original four source observations.
/// Visit each valid frame in the original order, including repeated frames.
pub(super) fn nearby(t: &TrackAnalysis, sec: f32, mut visit: impl FnMut(Power)) {
    let Some(stems) = &t.stems else { return };
    with_track(t, |values| {
        for i in 0..4 {
            if let Some((_, power)) = sample(&stems.frames, sec - 0.15 + 0.1 * i as f32, values) {
                visit(power);
            }
        }
    });
}

/// The reference depends on the immutable recording, never on candidate lanes.
/// Initialization only samples frames; it cannot launch or wait for nested jobs.
pub(super) fn reference(t: &TrackAnalysis) -> Option<f32> {
    VALUES
        .with(|v| {
            v.borrow().as_ref().and_then(|v| {
                v.iter()
                    .find(|(key, _)| *key == t as *const _ as usize)
                    .map(|(_, values)| {
                        *values.reference.get_or_init(|| super::measure_reference(t))
                    })
            })
        })
        .unwrap_or_else(|| super::measure_reference(t))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mixless_protocol::SectionLabel;

    fn track(id: i64, gain: f32) -> TrackAnalysis {
        let mut t = crate::tests::track(id, 120., "8A", SectionLabel::Intro, 32, 0.8, 0.2);
        t = crate::stem_regressions::with_stems(t);
        for f in &mut t.stems.as_mut().unwrap().frames {
            for db in f.band_db.iter_mut().flatten() {
                *db += gain;
            }
        }
        t
    }

    fn check(t: &TrackAnalysis) {
        assert_eq!(
            reference(t).map(f32::to_bits),
            super::super::measure_reference(t).map(f32::to_bits),
        );
        for sec in [
            -1.,
            0.,
            0.125,
            2.,
            t.duration_sec - 0.001,
            t.duration_sec,
            f32::NAN,
        ] {
            let expected = t.stems.as_ref().and_then(|s| s.frame_at(sec)).map(measure);
            assert_eq!(frame(t, sec).map(|(_, p)| p), expected);
            let foreground = t.stems.as_ref().and_then(|stems| {
                if sec >= t.duration_sec {
                    return Some(0f32);
                }
                let f = stems.frame_at(sec)?;
                Some(
                    f.band_db[0]
                        .iter()
                        .chain(&f.band_db[2][1..])
                        .map(|db| 10f32.powf(*db / 10.))
                        .sum::<f32>(),
                )
            });
            assert_eq!(
                super::super::unmixed_foreground(t, sec).map(f32::to_bits),
                foreground.map(f32::to_bits)
            );
            let expected: Vec<_> = (0..4)
                .filter_map(|i| t.stems.as_ref()?.frame_at(sec - 0.15 + 0.1 * i as f32))
                .map(measure)
                .collect();
            let mut actual = Vec::new();
            nearby(t, sec, |power| actual.push(power));
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn shared_power_keeps_exact_frames_and_restores_nested_scopes() {
        let a = track(1, 0.);
        let mut b = track(1, 6.); // Same library ID, different immutable evidence.
                                  // Gaps must remain missing observations, not borrow an adjacent frame.
        b.stems.as_mut().unwrap().frames.remove(0);
        let missing = crate::tests::track(2, 120., "8A", SectionLabel::Intro, 32, 0.8, 0.2);
        let evidence = Evidence::new(&a, &missing);
        std::thread::scope(|threads| {
            for _ in 0..4 {
                threads.spawn(|| {
                    let _scope = evidence.enter();
                    check(&a);
                    check(&b);
                    check(&missing);
                    let reused = Evidence::new(&a, &b);
                    assert!(Arc::ptr_eq(&evidence.0[0].1, &reused.0[0].1));
                    {
                        let _nested = reused.enter();
                        check(&a);
                        check(&b);
                    }
                    check(&a);
                    check(&b);
                });
            }
        });
        assert!(VALUES.with(|v| v.borrow().is_none()));
        assert!(evidence.0.iter().all(|(_, p)| p.reference.get().is_some()));
    }

    #[test]
    fn edited_analysis_cannot_reuse_a_finished_scope() {
        let mut a = track(1, 0.);
        let old = frame(&a, 0.).unwrap().1;
        let old_reference = reference(&a);
        {
            let _scope = Evidence::new(&a, &a).enter();
            check(&a);
        }
        for frame in &mut a.stems.as_mut().unwrap().frames {
            for db in frame.band_db.iter_mut().flatten() {
                *db += 6.;
            }
        }
        let _scope = Evidence::new(&a, &a).enter();
        check(&a);
        assert_ne!(frame(&a, 0.).unwrap().1, old);
        assert_ne!(reference(&a), old_reference);
    }

    #[test]
    fn missing_reference_is_cached_without_borrowing_foreign_evidence() {
        let mut a = track(1, 0.);
        a.bars.clear();
        let mut b = track(1, 0.);
        let evidence = Evidence::new(&a, &a);
        {
            let _scope = evidence.enter();
            assert_eq!(reference(&a), None);
            assert_eq!(evidence.0[0].1.reference.get(), Some(&None));
            assert!(reference(&b).is_some());
            // A same-ID foreign track is not retained by this immutable scope.
            b.bars.clear();
            assert_eq!(reference(&b), None);
        }
        assert!(VALUES.with(|v| v.borrow().is_none()));
    }

    #[test]
    fn nearby_samples_preserve_order_repetitions_gaps_and_exact_powers() {
        let mut a = track(1, 0.);
        let b = track(1, 6.);
        let frames = &mut a.stems.as_mut().unwrap().frames;
        for (i, f) in frames.iter_mut().enumerate() {
            for (j, db) in f.band_db.iter_mut().flatten().enumerate() {
                *db = -90. + ((i * 17 + j * 23) % 900) as f32 / 10.;
            }
        }
        frames.remove(17);
        for scoped in [false, true] {
            let evidence = Evidence::new(&a, &a);
            let _scope = scoped.then(|| evidence.enter());
            for t in [&a, &b] {
                for i in -32..4096 {
                    let sec = i as f32 / 64.;
                    let expected: Vec<_> = (0..4)
                        .filter_map(|j| t.stems.as_ref()?.frame_at(sec - 0.15 + 0.1 * j as f32))
                        .map(|f| measure(f).map(|row| row.map(f32::to_bits)))
                        .collect();
                    let mut actual = Vec::new();
                    nearby(t, sec, |power| {
                        actual.push(power.map(|row| row.map(f32::to_bits)))
                    });
                    assert_eq!(actual, expected, "scoped={scoped}, time={sec}");
                }
            }
        }
    }
}
