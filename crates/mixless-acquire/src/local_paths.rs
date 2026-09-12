//! Expand local file/folder selections on the import worker, never the UI thread.
use std::{collections::HashSet, path::PathBuf};

pub fn collect_audio(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<String>) {
    let mut files = Vec::new();
    let errors = visit_audio(paths, |path| files.push(path));
    (files, errors)
}

/// Deterministic discovery with incremental publication; no audio file is opened.
pub fn visit_audio(paths: &[PathBuf], mut found: impl FnMut(PathBuf)) -> Vec<String> {
    let mut pending: Vec<_> = paths.iter().rev().cloned().collect();
    let mut seen = HashSet::new();
    let mut errors = Vec::new();
    while let Some(path) = pending.pop() {
        let canonical = match path.canonicalize() {
            Ok(path) => path,
            Err(error) => {
                errors.push(format!("{}: {error}", path.display()));
                continue;
            }
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }
        if canonical.is_dir() {
            match std::fs::read_dir(&canonical) {
                Ok(entries) => {
                    let mut children = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(entry) => children.push(entry.path()),
                            Err(error) => errors.push(format!("{}: {error}", canonical.display())),
                        }
                    }
                    children.sort();
                    pending.extend(children.into_iter().rev());
                }
                Err(error) => errors.push(format!("{}: {error}", canonical.display())),
            }
        } else if canonical.is_file()
            && canonical
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| {
                    matches!(
                        e.to_ascii_lowercase().as_str(),
                        "wav" | "mp3" | "flac" | "aiff" | "aif" | "ogg" | "m4a" | "aac"
                    )
                })
        {
            found(canonical);
        }
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recursive_selection_is_sorted_deduplicated_and_filters_non_audio() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("album");
        std::fs::create_dir(&sub).unwrap();
        let song = sub.join("song.FLAC");
        std::fs::write(&song, b"fixture").unwrap();
        std::fs::write(sub.join("cover.jpg"), b"fixture").unwrap();
        let (files, errors) = collect_audio(&[dir.path().into(), song.clone(), sub]);
        assert_eq!(files, vec![song.canonicalize().unwrap()]);
        assert!(errors.is_empty());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_cycles_terminate_and_missing_paths_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(dir.path(), dir.path().join("cycle")).unwrap();
        let (files, errors) = collect_audio(&[dir.path().into(), dir.path().join("missing")]);
        assert!(files.is_empty());
        assert_eq!(errors.len(), 1);
    }
}
