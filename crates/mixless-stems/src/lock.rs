use crate::{check, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    time::Duration,
};

/// Advisory process lock. Dropping the file releases it after cancellation/crash.
pub fn acquire(path: &Path, active: &impl Fn() -> bool) -> Result<File> {
    acquire_mode(path, active, false)
}

/// Inference jobs share cache ownership; cleanup still requires exclusivity.
pub fn acquire_shared(path: &Path, active: &impl Fn() -> bool) -> Result<File> {
    acquire_mode(path, active, true)
}

fn acquire_mode(path: &Path, active: &impl Fn() -> bool, shared: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    loop {
        check(active)?;
        let result = if shared {
            fs2::FileExt::try_lock_shared(&file)
        } else {
            fs2::FileExt::try_lock_exclusive(&file)
        };
        match result {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25))
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parallel_publishers_share_ownership_but_cleanup_requires_exclusivity() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let a = acquire_shared(&path, &|| true).unwrap();
        let b = acquire_shared(&path, &|| true).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_millis(50);
        assert!(matches!(
            acquire(&path, &|| std::time::Instant::now() < deadline),
            Err(crate::Error::Cancelled)
        ));
        drop(a);
        assert!(acquire(&path, &|| false).is_err());
        drop(b);
        assert!(acquire(&path, &|| true).is_ok());
    }
    #[test]
    fn a_second_worker_can_cancel_while_waiting_for_cache_ownership() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lock");
        let first = acquire(&path, &|| true).unwrap();
        assert!(matches!(
            acquire(&path, &|| false),
            Err(crate::Error::Cancelled)
        ));
        drop(first);
        assert!(acquire(&path, &|| true).is_ok());
    }
}
