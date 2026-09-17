//! Preserve an incompatible library before opening a fresh database.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub(super) fn quarantine(path: &Path) -> io::Result<PathBuf> {
    quarantine_with(path, |from, to| fs::rename(from, to))
}

fn quarantine_with(
    path: &Path,
    mut move_file: impl FnMut(&Path, &Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Library has no parent directory"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("Library has no filename"))?;
    // Persist before moving any user data. Even a failed rollback must never
    // trigger TempDir cleanup of the only remaining copy of a journal.
    let backup = tempfile::Builder::new()
        .prefix(&format!("{}.unsupported-", name.to_string_lossy()))
        .tempdir_in(parent)?
        .keep();
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    let result = (|| {
        for suffix in ["-wal", "-shm", ""] {
            let mut filename = name.to_os_string();
            filename.push(suffix);
            let from = parent.join(&filename);
            match fs::symlink_metadata(&from) {
                Err(e) if !suffix.is_empty() && e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
                Ok(_) => {}
            }
            let to = backup.join(filename);
            move_file(&from, &to)?;
            moved.push((from, to));
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut rollback_error = None;
        for (from, to) in moved.into_iter().rev() {
            if let Err(e) = move_file(&to, &from) {
                rollback_error = Some(e);
            }
        }
        if let Some(rollback) = rollback_error {
            return Err(io::Error::other(format!(
                "Backup failed: {error}; restoring original files failed: {rollback}. Remaining files are preserved in {}",
                backup.display()
            )));
        }
        let _ = fs::remove_dir(&backup); // only an empty directory
        return Err(error);
    }
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn files(root: &Path) -> PathBuf {
        for name in ["library.db", "library.db-wal", "library.db-shm"] {
            fs::write(root.join(name), name.as_bytes()).unwrap();
        }
        root.join("library.db")
    }
    fn check(root: &Path) {
        for name in ["library.db", "library.db-wal", "library.db-shm"] {
            assert_eq!(fs::read(root.join(name)).unwrap(), name.as_bytes());
        }
    }
    #[test]
    fn repeated_backups_preserve_database_and_journals_without_collisions() {
        let root = tempfile::tempdir().unwrap();
        let first = quarantine(&files(root.path())).unwrap();
        let second = quarantine(&files(root.path())).unwrap();
        assert_ne!(first, second);
        check(&first);
        check(&second);
        assert!(!root.path().join("library.db").exists());
    }
    #[test]
    fn failed_sidecar_or_database_move_restores_the_original_files() {
        for fail in ["library.db-shm", "library.db"] {
            let root = tempfile::tempdir().unwrap();
            let db = files(root.path());
            let result = quarantine_with(&db, |from, to| {
                if from.parent() == Some(root.path()) && from.file_name().unwrap() == fail {
                    Err(io::Error::other("injected move failure"))
                } else {
                    fs::rename(from, to)
                }
            });
            assert!(result.is_err());
            check(root.path());
            assert_eq!(fs::read_dir(root.path()).unwrap().count(), 3);
        }
    }
    #[test]
    fn failed_rollback_keeps_the_only_copy_in_the_backup_directory() {
        let root = tempfile::tempdir().unwrap();
        let db = files(root.path());
        let result = quarantine_with(&db, |from, to| {
            if from.file_name().unwrap() == "library.db" || to.parent() == Some(root.path()) {
                Err(io::Error::other("injected failure"))
            } else {
                fs::rename(from, to)
            }
        });
        assert!(result.is_err());
        assert!(db.exists());
        let backup = fs::read_dir(root.path())
            .unwrap()
            .flatten()
            .find(|e| e.path().is_dir())
            .unwrap()
            .path();
        assert_eq!(
            fs::read(backup.join("library.db-wal")).unwrap(),
            b"library.db-wal"
        );
        assert_eq!(
            fs::read(backup.join("library.db-shm")).unwrap(),
            b"library.db-shm"
        );
    }
}
