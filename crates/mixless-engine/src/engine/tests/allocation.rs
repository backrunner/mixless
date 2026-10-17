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
fn absolute_scrub_callback_does_not_allocate_during_repeated_jumps() {
    use super::*;
    let engine = test_engine(48_000);
    let mut rt = engine.rt.lock().unwrap();
    let mut out = [0.; 512];
    engine.shared.process_block(&mut rt, &mut out, 2);
    engine
        .dispatch(Command::SetJogTouch {
            deck: DeckId::A,
            touching: true,
        })
        .unwrap();
    for frame in [144_000, 24_000, 96_000, 48_000, 0] {
        engine
            .dispatch(Command::ScrubTo {
                deck: DeckId::A,
                frame,
            })
            .unwrap();
        let counts = measure(|| engine.shared.process_block(&mut rt, &mut out, 2));
        assert_eq!(
            counts, [0; 3],
            "alloc/dealloc/realloc in absolute scrub callback"
        );
        assert!(out.iter().all(|sample| *sample == 0.));
    }
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

#[test]
fn master_processors_do_not_allocate_during_live_parameter_changes() {
    use super::*;
    let engine = test_engine(48000);
    let mut settings = mixless_protocol::MasterChainSettings::default();
    settings.eq_enabled = true;
    settings.eq[2].gain_db = 6.;
    settings.compressor.enabled = true;
    settings.dynamics.enabled = true;
    engine
        .dispatch(Command::SetMasterChain { settings })
        .unwrap();
    let mut rt = engine.rt.lock().unwrap();
    let mut out = [0.; 512];
    settings.eq[2].frequency_hz = 400.;
    settings.dynamics.gate = true;
    settings.limiter.ceiling_db = -3.;
    engine
        .dispatch(Command::SetMasterChain { settings })
        .unwrap();
    let counts = measure(|| {
        for _ in 0..400 {
            engine.shared.process_block(&mut rt, &mut out, 2);
        }
    });
    assert_eq!(counts, [0; 3], "master callback heap operations");
}
