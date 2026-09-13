//! Counts Rust heap operations only; native stretcher allocation is not intercepted.
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
struct CountingAllocator;
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
thread_local! {
    static ACTIVE:Cell<bool>=const {Cell::new(false)};
    static COUNTS:Cell<[usize;3]>=const {Cell::new([0;3])};
}
fn mark(i: usize) {
    let _ = ACTIVE.try_with(|active| {
        if active.get() {
            let _ = COUNTS.try_with(|counts| {
                let mut c = counts.get();
                c[i] += 1;
                counts.set(c);
            });
        }
    });
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        mark(0);
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        mark(0);
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        mark(1);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        mark(2);
        unsafe { System.realloc(p, l, n) }
    }
}
pub(super) fn measure(f: impl FnOnce()) -> [usize; 3] {
    COUNTS.with(|c| c.set([0; 3]));
    ACTIVE.with(|a| a.set(true));
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            ACTIVE.with(|a| a.set(false));
        }
    }
    let reset = Reset;
    f();
    drop(reset);
    COUNTS.with(Cell::get)
}

#[test]
fn stem_callback_does_not_allocate_or_reclaim_pcm_on_seek_and_replacement() {
    use super::*;
    let engine = super::stems::stem_fixture(48000, false, true);
    for deck in [DeckId::A, DeckId::B] {
        engine
            .dispatch(Command::SetStemGain {
                deck,
                stem: mixless_protocol::StemKind::Vocals,
                value: 0.2,
            })
            .unwrap();
        engine
            .dispatch(Command::SetRate { deck, rate: 1.08 })
            .unwrap();
        engine.dispatch(Command::PlayPause { deck }).unwrap();
    }
    let mut rt = engine.rt.lock().unwrap();
    let mut out = [0.; 256];
    let counts = measure(|| {
        for _ in 0..200 {
            engine.shared.process_block(&mut rt, &mut out, 2);
        }
    });
    assert_eq!(counts, [0; 3], "alloc/dealloc/realloc in callback");
    engine.shared.decks[0].seek_cue(48000);
    let counts = measure(|| engine.shared.process_block(&mut rt, &mut out, 2));
    assert_eq!(counts, [0; 3]);
    engine.eject(DeckId::A);
    let counts = measure(|| engine.shared.process_block(&mut rt, &mut out, 2));
    assert_eq!(counts, [0; 3], "PCM destruction must remain on host");
}
