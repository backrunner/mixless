//! Single-flight self-updates. A ready update stays ready until restart.
use crate::state::AppCore;
use std::sync::{Arc, Mutex};
#[cfg(target_os = "macos")]
mod feed;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod network;

/// Shared updater slot on `AppCore`; `poll_update` renders it as a banner.
pub type Shared = Mutex<Option<Entry>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub status: Status,
    /// Distinguish consecutive checks even when the UI misses Checking and
    /// observes the same terminal status twice (for example, UpToDate).
    request: u64,
    /// Menu-triggered checks surface every outcome; launch checks stay quiet
    /// unless real work is happening.
    pub manual: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Checking,
    Downloading {
        version: String,
        percent: u8,
    },
    Installing {
        version: String,
    },
    /// Installed over the app; a restart finishes the update.
    Ready {
        version: String,
    },
    UpToDate,
    Failed(String),
}

/// Manual checks also make an already-running launch check visible.
pub fn start(core: Arc<AppCore>, manual: bool) {
    if !begin(&core.update, manual) {
        return;
    }
    std::thread::spawn(move || {
        let report = |status: Status| {
            if !matches!(status, Status::Downloading { .. }) {
                tracing::info!(?status, "update status");
            }
            publish(&core.update, status);
        };
        let status = run(&report).unwrap_or_else(Status::Failed);
        report(status);
    });
}

fn begin(shared: &Shared, manual: bool) -> bool {
    let mut slot = shared.lock().expect("update status");
    if let Some(entry) = slot.as_mut() {
        entry.manual |= manual;
        if matches!(
            entry.status,
            Status::Checking
                | Status::Downloading { .. }
                | Status::Installing { .. }
                | Status::Ready { .. }
        ) {
            return false;
        }
    }
    let request = slot.as_ref().map_or(1, |e| e.request.wrapping_add(1));
    *slot = Some(Entry {
        status: Status::Checking,
        request,
        manual,
    });
    true
}

fn publish(shared: &Shared, status: Status) {
    let mut slot = shared.lock().expect("update status");
    if let Some(entry) = slot.as_mut() {
        entry.status = status;
    }
}

/// Dev builds and unpackaged runs never launch a background download.
pub fn launch(core: &Arc<AppCore>) {
    if env!("MIXLESS_CHANNEL") != "dev" && crate::self_install::current_bundle().is_some() {
        start(core.clone(), false);
    }
}

#[cfg(target_os = "macos")]
fn run(report: &dyn Fn(Status)) -> Result<Status, String> {
    macos::update(report)
}

#[cfg(not(target_os = "macos"))]
fn run(_report: &dyn Fn(Status)) -> Result<Status, String> {
    Err("Updates are only supported on macOS".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_check_remains_visible_through_background_reports() {
        let shared = Mutex::new(None);
        assert!(begin(&shared, false));
        assert!(!begin(&shared, true));
        for status in [
            Status::Downloading {
                version: "1.2.3".into(),
                percent: 50,
            },
            Status::Failed("interrupted".into()),
        ] {
            publish(&shared, status);
            assert!(shared.lock().unwrap().as_ref().unwrap().manual);
        }
        assert!(begin(&shared, true)); // a failed check remains retryable
    }

    #[test]
    fn ready_update_cannot_be_replaced_by_a_second_check() {
        let shared = Mutex::new(None);
        assert!(begin(&shared, false));
        let ready = Status::Ready {
            version: "1.2.3".into(),
        };
        publish(&shared, ready.clone());
        assert!(!begin(&shared, true));
        assert_eq!(shared.lock().unwrap().as_ref().unwrap().status, ready);
        assert!(shared.lock().unwrap().as_ref().unwrap().manual);
    }

    #[test]
    fn repeated_fast_checks_refresh_an_expired_notice() {
        let shared = Mutex::new(None);
        assert!(begin(&shared, true));
        publish(&shared, Status::UpToDate);
        let previous = shared.lock().unwrap().clone();
        assert!(begin(&shared, true));
        publish(&shared, Status::UpToDate);
        // poll_update must see a change even if neither Checking was painted.
        assert_ne!(*shared.lock().unwrap(), previous);
    }
}
