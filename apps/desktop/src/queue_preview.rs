//! Full native workspace with the production worker and an offline audio clock.
//! Opt-in development harness; it never opens the ordinary library or audio device.
use crate::state::AppCore;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

fn enabled() -> bool {
    std::env::args().nth(1).as_deref() == Some("--preview-automix-queue")
}

pub(crate) fn check_environment() {
    if !enabled() {
        return;
    }
    let path = std::path::PathBuf::from(
        std::env::var_os("MIXLESS_DATA_DIR").expect("isolated preview data"),
    );
    assert!(path.is_absolute() && path.starts_with(std::env::temp_dir()));
    assert_eq!(std::env::var("MIXLESS_OFFLINE").as_deref(), Ok("1"));
    assert_eq!(std::env::var("MIXLESS_MODELS_OFFLINE").as_deref(), Ok("1"));
}

pub(crate) fn start(core: &Arc<AppCore>) {
    if !enabled() {
        return;
    }
    let core = core.clone();
    std::thread::spawn(move || {
        while !core.shutting_down.load(Ordering::Acquire) {
            core.engine
                .render_offline((core.engine.sample_rate() / 100) as usize);
            std::thread::sleep(Duration::from_millis(10));
        }
    });
}

/// A file gate holds only the real optimizer, leaving queue playback active.
pub(crate) fn wait_for_best(active: impl Fn() -> bool) {
    if !enabled() {
        return;
    }
    if let Some(path) = std::env::var_os("MIXLESS_PREVIEW_BEST_GATE") {
        while std::path::Path::new(&path).exists() && active() {
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}
