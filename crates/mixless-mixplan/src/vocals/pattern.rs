//! Memoize local evidence only while the planner holds immutable input borrows.
//! Nothing survives a call: pointer reuse, reanalysis and edited test fixtures
//! cannot inherit another source's vocal classification.
use mixless_protocol::{TrackAnalysis, VocalPattern};
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

struct Cache {
    tracks: [usize; 2],
    values: HashMap<(usize, usize), Option<VocalPattern>>,
}
thread_local! { static CACHE: RefCell<Option<Cache>> = const { RefCell::new(None) }; }

pub(crate) struct Scope<'a> {
    previous: Option<Cache>,
    // Restoring thread-local state must happen on the thread that entered it.
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

pub(crate) fn at(t: &TrackAnalysis, sec: f32) -> Option<VocalPattern> {
    let stems = t.stems.as_ref()?;
    let i = stems.frames.partition_point(|f| f.end_sec <= sec);
    let frame = stems.frames.get(i).filter(|f| f.start_sec <= sec)?;
    let key = (t as *const _ as usize, i);
    if let Some(value) = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|c| c.values.get(&key).copied())
    }) {
        return value;
    }
    let value = mixless_protocol::vocal_pattern(t, (frame.start_sec + frame.end_sec) * 0.5);
    CACHE.with(|c| {
        if let Some(c) = c
            .borrow_mut()
            .as_mut()
            .filter(|c| c.tracks.contains(&key.0))
        {
            c.values.insert(key, value);
        }
    });
    value
}

/// A pattern grants rhythmic fades, not permission to seek into a held slice.
pub(crate) fn boundary(t: &TrackAnalysis, sec: f32) -> bool {
    let Some(pattern) = at(t, sec) else {
        return false;
    };
    let stems = t.stems.as_ref().unwrap();
    if stems.note_crossing(sec, mixless_protocol::StemKind::Vocals) {
        return false;
    }
    let Some(frame) = stems.frame_at(sec) else {
        return false;
    };
    let first = stems
        .frames
        .partition_point(|f| f.end_sec <= sec - pattern.period_sec);
    let peak = stems.frames[first..]
        .iter()
        .take_while(|f| f.start_sec < sec + pattern.period_sec)
        .map(|f| f.rms[0])
        .fold(0., f32::max);
    frame.rms[0] <= peak * 0.5
        || stems.notes.iter().any(|n| {
            n.stem == mixless_protocol::StemKind::Vocals
                && n.confidence >= 0.45
                && (n.start_sec - sec).abs() < 0.04
        })
}

pub(crate) fn window(t: &TrackAnalysis, start: f32, end: f32) -> bool {
    if !start.is_finite() || !end.is_finite() || end < start + 0.05 {
        return false;
    }
    let steps = ((end - start) / 0.25).ceil() as usize;
    (0..=steps).all(|i| at(t, start + (end - start) * i as f32 / steps as f32).is_some())
}
