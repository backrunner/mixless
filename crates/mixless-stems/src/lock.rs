use crate::{check, Result};
use std::{
    fs::{File, OpenOptions},
    path::Path,
    time::Duration,
};

/// Advisory process lock. Dropping the file releases it after cancellation/crash.
pub fn acquire(path: &Path, active: &impl Fn() -> bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    loop {
        check(active)?;
        match fs2::FileExt::try_lock_exclusive(&file) {
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
