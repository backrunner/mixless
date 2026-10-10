use super::*;
pub(super) fn worker() -> (
    tempfile::TempDir,
    Worker,
    mpsc::Receiver<Event>,
    Arc<Mutex<Vec<ControlEvent>>>,
) {
    let temp = tempfile::tempdir().unwrap();
    let events = Arc::new(Mutex::new(vec![]));
    let collected = events.clone();
    let shared = Arc::new(RuntimeShared {
        config: Mutex::new(MidiConfig::default()),
        store: Arc::new(Mutex::new(
            PresetStore::open(&temp.path().join("midi.json")).unwrap(),
        )),
        input: Arc::new(Mutex::new(InputState::default())),
        devices: Mutex::new(vec![]),
        last: Mutex::new(None),
        handler: Mutex::new(Some(Arc::new(move |e| {
            collected.lock().unwrap().push(e);
            true
        }))),
        snapshot: Mutex::new(None),
        notify: Mutex::new(None),
        revision: AtomicU64::new(1),
        latencies: Mutex::new(vec![]),
        output_errors: Mutex::new(HashMap::new()),
        adapter_errors: Mutex::new(HashMap::new()),
        recording: Mutex::new(None),
    });
    let (tx, rx) = mpsc::sync_channel(8192);
    (temp, Worker::new(shared, tx), rx, events)
}
