//! Mirror the updater worker's status into a banner.
use super::*;
use crate::update::{Entry, Status};
use std::time::{Duration, Instant};

/// Transient notices (checking, up-to-date, failures) fade after this.
const NOTICE_TTL: Duration = Duration::from_secs(8);

impl UiState {
    pub(super) fn poll_update(&mut self) -> bool {
        let entry = self.core.update.lock().expect("update status").clone();
        if entry == self.update_entry {
            if !self.update_notice.is_empty()
                && !self.update_notice_hold
                && self
                    .update_notice_at
                    .is_some_and(|at| at.elapsed() > NOTICE_TTL)
            {
                self.update_notice = "".into();
                return true;
            }
            return false;
        }
        self.update_entry = entry.clone();
        let Some(entry) = entry else {
            self.update_notice = "".into();
            return true;
        };
        // Launch checks stay silent for the routine path: checking, already
        // up to date, and failures nobody saw coming. Real work — a download
        // in flight, an install, a ready update — always surfaces.
        let quiet = !entry.manual
            && !matches!(
                entry.status,
                Status::Downloading { .. } | Status::Installing { .. } | Status::Ready { .. }
            )
            && self.update_notice.is_empty();
        if quiet {
            return true;
        }
        // In-flight states hold until the next transition; only terminal
        // notices (up-to-date, failed) fade on the timer.
        self.update_notice_hold = !matches!(entry.status, Status::UpToDate | Status::Failed(_));
        self.update_notice_at = Some(Instant::now());
        self.update_notice = notice(&entry).into();
        true
    }
}

fn notice(entry: &Entry) -> String {
    match &entry.status {
        Status::Checking => "Checking for updates…".into(),
        Status::Downloading { version, percent } => {
            format!("Downloading mixless {version} — {percent}%")
        }
        Status::Installing { version } => format!("Installing mixless {version}…"),
        Status::Ready { version } => {
            format!("mixless {version} installed — restart to finish updating")
        }
        Status::UpToDate => format!("mixless {} is up to date", env!("CARGO_PKG_VERSION")),
        Status::Failed(error) => format!("Update failed: {error}"),
    }
}
