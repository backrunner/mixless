//! Opt-in offline diagnostics; absent from desktop builds and audio callbacks.
pub(super) fn write(name: &str, value: impl FnOnce() -> serde_json::Value) {
    let Some(directory) = std::env::var_os("MIXLESS_ORDER_AUDIT") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    std::fs::create_dir_all(&directory).expect("create order audit directory");
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec(&value()).expect("serialize order audit"),
    )
    .expect("write order audit");
}
