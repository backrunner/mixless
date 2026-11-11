use super::*;

#[test]
fn full_or_disconnected_queue_drops_only_diagnostics() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let observer = Arc::new(Observer {
        sender,
        submitted: AtomicU64::new(0),
        dropped: AtomicU64::new(0),
        skipped_inputs: AtomicU64::new(0),
        cpu_scores: AtomicU64::new(0),
        cpu_nanos: AtomicU64::new(0),
        cache_hits: AtomicU64::new(0),
        dropped_queries: AtomicU64::new(0),
        mode: Mode::BestEffort,
        queue_wait_nanos: AtomicU64::new(0),
    });
    let capture = Capture {
        input: Arc::new(Input {
            frames: vec![],
            tonality: 0,
        }),
        observer: Arc::clone(&observer),
        pending: Mutex::new(Pending::default()),
    };
    capture.observe([[0.; 16]].into_iter(), &[Some(1.)]);
    capture.flush(&mut capture.pending.lock().unwrap());
    capture.observe([[0.; 16]].into_iter(), &[None]);
    capture.flush(&mut capture.pending.lock().unwrap());
    assert_eq!(observer.submitted.load(Ordering::Relaxed), 1);
    assert_eq!(observer.dropped.load(Ordering::Relaxed), 1);
    let Message::Job(job) = receiver.recv().unwrap() else {
        panic!("missing CPU result");
    };
    assert_eq!(job.cpu, vec![Some(1.)]);
    drop(receiver);
    capture.observe([[0.; 16]].into_iter(), &[Some(2.)]);
    capture.flush(&mut capture.pending.lock().unwrap());
    assert_eq!(observer.dropped.load(Ordering::Relaxed), 2);
}

#[test]
#[cfg(feature = "trace")]
fn tracing_never_starts_gpu_work() {
    let (value, report) = compare(Mode::Complete, || 42);
    assert_eq!(value, 42);
    assert_eq!(report["status"], "disabled_for_trace");
}

#[test]
#[ignore = "requires a Metal device; run with --test-threads=1"]
#[cfg(all(target_os = "macos", not(feature = "trace")))]
fn metal_session_restores_observer_after_panic_and_nested_calls() {
    let panic =
        std::panic::catch_unwind(|| compare(Mode::Complete, || panic!("deliberate planner panic")));
    assert!(panic.is_err());
    assert!(ACTIVE.lock().unwrap().is_none());
    let (value, report) = compare(Mode::Complete, || {
        let (inner, status) = compare(Mode::Complete, || 7);
        assert_eq!(status["status"], "already_active");
        inner + 1
    });
    assert_eq!(value, 8);
    assert_eq!(report["status"], "complete");
    assert_eq!(report["submitted_jobs"], 0);
    assert!(ACTIVE.lock().unwrap().is_none());
}
