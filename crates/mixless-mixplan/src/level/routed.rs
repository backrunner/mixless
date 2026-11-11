//! Reuse routed observations only while the exact plan and analyses are borrowed.
//! Rendering/finishing may edit every lane: nothing survives this immutable scope.
use mixless_protocol::{MixPlan, TrackAnalysis};
use std::{cell::RefCell, collections::HashMap, marker::PhantomData, rc::Rc};

#[derive(Clone, Copy)]
pub(super) struct Deck {
    pub source: f32,
    pub selected: [f32; 4],
    pub multiplier: [f32; 3],
}
pub(super) type Controls = [Option<Deck>; 2];
type Foreground = ([f32; 2], [f32; 2]);
#[derive(Default)]
struct Sample {
    controls: Option<Controls>,
    full: Option<[f32; 2]>,
    foreground: Option<Foreground>,
}
struct Cache {
    identity: [usize; 3],
    values: HashMap<u32, Sample>,
}
thread_local! { static CURRENT: RefCell<Option<Cache>> = const { RefCell::new(None) }; }
struct Scope<'a>(
    Option<Cache>,
    PhantomData<(&'a MixPlan, &'a TrackAnalysis, Rc<()>)>,
);
impl Drop for Scope<'_> {
    fn drop(&mut self) {
        CURRENT.with(|c| c.replace(self.0.take()));
    }
}
fn identity(p: &MixPlan, a: &TrackAnalysis, b: &TrackAnalysis) -> [usize; 3] {
    [
        p as *const _ as usize,
        a as *const _ as usize,
        b as *const _ as usize,
    ]
}

pub(crate) fn with_plan<T>(
    p: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    f: impl FnOnce() -> T,
) -> T {
    let _scope = Scope(
        CURRENT.with(|c| {
            c.replace(Some(Cache {
                identity: identity(p, a, b),
                values: HashMap::new(),
            }))
        }),
        PhantomData,
    );
    f()
}

fn cached<T: Copy>(
    p: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    u: f32,
    field: impl Fn(&mut Sample) -> &mut Option<T>,
    compute: impl FnOnce() -> T,
) -> T {
    let id = identity(p, a, b);
    let hit = CURRENT.with(|c| {
        let mut c = c.borrow_mut();
        c.as_mut()
            .filter(|c| c.identity == id)
            .and_then(|c| c.values.get_mut(&u.to_bits()))
            .and_then(|s| *field(s))
    });
    if let Some(value) = hit {
        return value;
    }
    let value = compute();
    CURRENT.with(|c| {
        if let Some(c) = c.borrow_mut().as_mut().filter(|c| c.identity == id) {
            // This is an eviction limit, never a sample or candidate budget.
            if c.values.len() >= 8192 && !c.values.contains_key(&u.to_bits()) {
                c.values.clear();
            }
            *field(c.values.entry(u.to_bits()).or_default()) = Some(value);
        }
    });
    value
}

pub(super) fn full(
    p: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    u: f32,
    f: impl FnOnce() -> [f32; 2],
) -> [f32; 2] {
    cached(p, a, b, u, |s| &mut s.full, f)
}
pub(super) fn foreground(
    p: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    u: f32,
    f: impl FnOnce() -> Foreground,
) -> Foreground {
    cached(p, a, b, u, |s| &mut s.foreground, f)
}
pub(super) fn controls(
    p: &MixPlan,
    a: &TrackAnalysis,
    b: &TrackAnalysis,
    u: f32,
    f: impl FnOnce() -> Controls,
) -> Controls {
    cached(p, a, b, u, |s| &mut s.controls, f)
}

#[cfg(test)]
mod tests;
