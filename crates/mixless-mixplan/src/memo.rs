//! Completed plans shared only within an immutable planning request.
//! Never wait for an in-flight plan: a Rayon worker waiting on another worker's
//! nested search could starve the pool. Concurrent cold misses may compute twice.
use crate::{PlanContext, PlannerOptions};
use mixless_protocol::{MixPlan, TrackAnalysis};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    marker::PhantomData,
    rc::Rc,
    sync::{Arc, Mutex},
};

mod key;
mod weight;
use key::Key;

const MAX_ENTRIES: usize = 256;
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct Values {
    entries: HashMap<Key, (Arc<Vec<MixPlan>>, usize)>,
    fifo: VecDeque<Key>,
    bytes: usize,
    peak_bytes: usize,
    hits: usize,
    misses: usize,
    evictions: usize,
}

struct Shared {
    tracks: HashSet<usize>,
    values: Mutex<Values>,
    sources: crate::evidence::Sources,
}
thread_local! { static CURRENT: RefCell<Option<Arc<Shared>>> = const { RefCell::new(None) }; }

/// The input borrows remain live until all synchronous worker jobs have joined.
/// Pointer identity is never reused across independent planning requests.
pub(crate) struct Session<'a> {
    shared: Arc<Shared>,
    _tracks: Vec<&'a TrackAnalysis>,
    owner: bool,
}

impl<'a> Session<'a> {
    pub(crate) fn new(tracks: &[&'a TrackAnalysis]) -> Self {
        let existing = CURRENT.with(|c| {
            c.borrow()
                .as_ref()
                .filter(|s| {
                    tracks
                        .iter()
                        .all(|t| s.tracks.contains(&(*t as *const _ as usize)))
                })
                .cloned()
        });
        let owner = existing.is_none();
        Self {
            shared: existing.unwrap_or_else(|| {
                Arc::new(Shared {
                    tracks: tracks.iter().map(|t| *t as *const _ as usize).collect(),
                    values: Mutex::new(Values::default()),
                    sources: crate::evidence::Sources::default(),
                })
            }),
            _tracks: tracks.to_vec(),
            owner,
        }
    }

    pub(crate) fn enter(&self) -> Scope<'_> {
        install(Some(Arc::clone(&self.shared)))
    }
}

pub(crate) struct Scope<'a> {
    previous: Option<Arc<Shared>>,
    _borrow: PhantomData<(&'a (), Rc<()>)>,
}
fn install<'a>(shared: Option<Arc<Shared>>) -> Scope<'a> {
    Scope {
        previous: CURRENT.with(|c| c.replace(shared)),
        _borrow: PhantomData,
    }
}
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        CURRENT.with(|c| c.replace(self.previous.take()));
    }
}

/// Only the synchronous map helper may carry this token to another worker.
/// `with_workers` owns it until every worker has joined; it cannot escape.
pub(crate) struct Workers(Option<Arc<Shared>>);
impl Workers {
    pub(crate) fn enter(&self) -> Scope<'_> {
        install(self.0.clone())
    }
}
pub(crate) fn with_workers<T>(f: impl FnOnce(&Workers) -> T) -> T {
    let workers = Workers(CURRENT.with(|c| c.borrow().clone()));
    f(&workers)
}

pub(crate) fn sources(a: &TrackAnalysis, b: &TrackAnalysis) -> Option<crate::evidence::Sources> {
    if cfg!(feature = "trace") {
        return None;
    }
    CURRENT.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|s| {
                [a, b]
                    .iter()
                    .all(|t| s.tracks.contains(&(*t as *const _ as usize)))
            })
            .map(|s| s.sources.clone())
    })
}

pub(crate) fn plans(
    ctx: &PlanContext<'_>,
    options: &PlannerOptions,
    collect: bool,
    compute: impl FnOnce() -> Vec<MixPlan>,
) -> Vec<MixPlan> {
    // Trace collectors must still observe every rejection from the real search.
    let shared = (!cfg!(feature = "trace"))
        .then(|| CURRENT.with(|c| c.borrow().clone()))
        .flatten();
    let Some(shared) = shared.filter(|s| {
        [ctx.outgoing, ctx.incoming]
            .iter()
            .all(|t| s.tracks.contains(&(*t as *const _ as usize)))
    }) else {
        return compute();
    };
    let key = Key::new(ctx, options, collect);
    let hit = {
        let mut values = shared.values.lock().unwrap();
        let hit = values.entries.get(&key).map(|(p, _)| Arc::clone(p));
        if hit.is_some() {
            values.hits += 1;
        } else {
            values.misses += 1;
        }
        hit
    };
    if let Some(plans) = hit {
        return plans.as_ref().clone();
    }
    let plans = compute();
    let bytes = weight::plans(&plans) + key.bytes() * 2 + 128;
    if bytes <= MAX_BYTES {
        let stored = Arc::new(plans.clone());
        let mut values = shared.values.lock().unwrap();
        if !values.entries.contains_key(&key) {
            while values.entries.len() >= MAX_ENTRIES || values.bytes + bytes > MAX_BYTES {
                let old = values.fifo.pop_front().expect("cache budget has an entry");
                if let Some((_, size)) = values.entries.remove(&old) {
                    values.bytes -= size;
                    values.evictions += 1;
                }
            }
            values.fifo.push_back(key.clone());
            values.entries.insert(key, (stored, bytes));
            values.bytes += bytes;
            values.peak_bytes = values.peak_bytes.max(values.bytes);
        }
    }
    plans
}

impl Drop for Session<'_> {
    fn drop(&mut self) {
        if !self.owner {
            return;
        }
        #[cfg(feature = "audit")]
        if let Some(path) = std::env::var_os("MIXLESS_PLAN_CACHE_STATS") {
            use std::io::Write;
            let values = self.shared.values.lock().unwrap();
            let stats = serde_json::json!({"hits":values.hits,"misses":values.misses,"entries":values.entries.len(),"evictions":values.evictions,"peak_bytes":values.peak_bytes});
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{stats}");
            }
        }
    }
}

#[cfg(test)]
mod tests;
