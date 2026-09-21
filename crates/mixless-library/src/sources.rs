//! Recover moved audio without changing track identity or guessing by title.
use super::*;
use std::collections::{BTreeSet, HashSet, VecDeque};

impl Library {
    /// Worker-thread only. Search known track directories, imported folders and
    /// managed downloads. A filename is a hint; only identical bytes auto-link.
    pub fn resolve_track_file(&self, id: TrackId) -> Result<Track, LibraryError> {
        let track = self.get_track(id)?;
        match fs::metadata(&track.path) {
            Ok(meta) if meta.is_file() => return Ok(track),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        if track.content_hash.is_empty() {
            return Err(LibraryError::MissingFile(track.path));
        }
        let mut roots = BTreeSet::new();
        for other in self.list_tracks()? {
            if let Some(parent) = Path::new(&other.path).parent() {
                roots.insert(parent.to_path_buf());
            }
        }
        for playlist in self.list_playlists()? {
            if let Some(folder) = playlist.folder_path {
                roots.insert(PathBuf::from(folder));
            }
        }
        roots.insert(self.db_path.with_file_name("acquired"));
        // A prior verification gives a size hint for renamed files. It never
        // substitutes for checking their full content hash.
        let fingerprint: Option<String> = self
            .conn
            .lock()
            .expect("library mutex")
            .query_row(
                "SELECT fingerprint FROM file_verification WHERE path=?1",
                [&track.path],
                |r| r.get(0),
            )
            .optional()?;
        let size = fingerprint.as_deref().and_then(|value| {
            value
                .strip_prefix("v1:")?
                .split(':')
                .nth(2)?
                .parse::<u64>()
                .ok()
        });
        let name = Path::new(&track.path).file_name();
        let mut named = BTreeSet::new();
        let mut renamed = BTreeSet::new();
        let mut visited = HashSet::new();
        let mut queue: VecDeque<_> = roots.into_iter().map(|p| (p, 0)).collect();
        let mut entries = 0;
        // Bound recovery work; never walk an entire disk or follow directory
        // symlinks. Remaining cases use the explicit file picker.
        while let Some((dir, depth)) = queue.pop_front() {
            if entries >= 10_000 {
                break;
            }
            let Ok(dir) = fs::canonicalize(dir) else {
                continue;
            };
            if !visited.insert(dir.clone()) {
                continue;
            }
            let Ok(children) = fs::read_dir(dir) else {
                continue;
            };
            for entry in children.flatten() {
                entries += 1;
                if entries > 10_000 {
                    break;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let path = entry.path();
                if kind.is_dir() && depth < 4 {
                    queue.push_back((path, depth + 1));
                } else if kind.is_file() {
                    if path.file_name() == name {
                        named.insert(path);
                    } else if size
                        .is_some_and(|size| entry.metadata().is_ok_and(|m| m.len() == size))
                    {
                        renamed.insert(path);
                    }
                }
            }
        }
        for candidate in named.into_iter().chain(renamed).take(128) {
            if self.verified_content_hash(&candidate).ok().as_deref() != Some(&track.content_hash) {
                continue;
            }
            match self.relink_track_file(&track, &candidate, &track.content_hash) {
                Ok(track) => return Ok(track),
                Err(LibraryError::SourceInUse) => continue,
                Err(error) => return Err(error),
            }
        }
        // Another worker may have resolved the same track while we searched.
        let current = self.get_track(id)?;
        if current.path != track.path && Path::new(&current.path).is_file() {
            return Ok(current);
        }
        Err(LibraryError::MissingFile(track.path))
    }

    /// The caller validates that a manually chosen replacement decodes before
    /// committing it. Preserve ID, ordering, playlist membership and user cues.
    pub fn relink_track_file(
        &self,
        expected: &Track,
        path: &Path,
        verified_hash: &str,
    ) -> Result<Track, LibraryError> {
        let path = fs::canonicalize(path)?;
        let hash = self.verified_content_hash(&path)?;
        if hash != verified_hash {
            return Err(LibraryError::SourceChanged);
        }
        let changed = hash != expected.content_hash;
        let meta = changed.then(|| read_meta(&path));
        let artwork = match &meta {
            Some(meta) => self
                .cache_artwork(&hash, meta.artwork.as_ref())?
                .unwrap_or_default(),
            None => String::new(),
        };
        let mtime = fs::metadata(&path)?
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let in_use: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM tracks WHERE path=?1 AND id<>?2)",
            params![path.to_string_lossy(), expected.id.0],
            |r| r.get(0),
        )?;
        if in_use {
            return Err(LibraryError::SourceInUse);
        }
        if tx.execute(
            "UPDATE tracks SET path=?1,mtime=?2 WHERE id=?3 AND path=?4 AND content_hash=?5",
            params![
                path.to_string_lossy(),
                mtime,
                expected.id.0,
                expected.path,
                expected.content_hash
            ],
        )? != 1
        {
            return Err(LibraryError::SourceChanged);
        }
        if let Some(meta) = meta {
            tx.execute("UPDATE tracks SET content_hash=?2,title=?3,artist=?4,album=?5,duration_ms=?6,isrc=?7,
                artwork_path=?8,analyzed=0,bpm=NULL,key=NULL,camelot=NULL WHERE id=?1",
                params![expected.id.0, hash, meta.title, meta.artist, meta.album, meta.duration_ms,
                    meta.isrc, artwork])?;
            for table in ["track_analysis", "track_waveforms", "cue_versions"] {
                tx.execute(
                    &format!("DELETE FROM {table} WHERE track_id=?1"),
                    [expected.id.0],
                )?;
            }
            tx.execute(
                "DELETE FROM cues WHERE track_id=?1 AND user_set=0",
                [expected.id.0],
            )?;
        }
        tx.commit()?;
        drop(conn);
        self.get_track(expected.id)
    }

    /// Remove library references only. Never delete the user's audio file.
    pub fn remove_track(&self, id: TrackId) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        // Otherwise ON DELETE SET NULL would turn these into ghost import rows.
        tx.execute("DELETE FROM import_items WHERE track_id=?1", [id.0])?;
        if tx.execute("DELETE FROM tracks WHERE id=?1", [id.0])? != 1 {
            return Err(LibraryError::NotFound);
        }
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Library, Track) {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        let source = dir.path().join("old/song.wav");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"original audio").unwrap();
        let id = library.import_file(&source).unwrap();
        library.set_cue(id, 7, 200, CueKind::In, true).unwrap();
        library
            .set_analysis_meta(id, Some(120.), None, None)
            .unwrap();
        let track = library.get_track(id).unwrap();
        (dir, library, track)
    }

    #[test]
    fn moved_file_in_another_tracks_directory_retains_identity_across_restarts() {
        let (dir, library, track) = fixture();
        let other = dir.path().join("new/other.wav");
        fs::create_dir_all(other.parent().unwrap()).unwrap();
        fs::write(&other, b"other audio").unwrap();
        let other_id = library.import_file(&other).unwrap();
        let playlist = library
            .replace_playlist("Set", &[other_id, track.id])
            .unwrap();
        let moved = dir.path().join("new/song.wav");
        fs::rename(&track.path, &moved).unwrap();
        let resolved = library.resolve_track_file(track.id).unwrap();
        assert_eq!(Path::new(&resolved.path), moved.canonicalize().unwrap());
        assert_eq!(resolved.id, track.id);
        assert_eq!(resolved.bpm, Some(120.));
        assert!(resolved.analyzed);
        assert_eq!(library.cues(track.id).unwrap()[0].frame, 200);
        assert_eq!(
            library
                .playlist_tracks(playlist)
                .unwrap()
                .iter()
                .map(|t| t.id)
                .collect::<Vec<_>>(),
            [other_id, track.id]
        );
        drop(library);
        let library = Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(library.get_track(track.id).unwrap().path, resolved.path);
    }

    #[test]
    fn renamed_file_is_found_in_an_imported_subfolder_and_wrong_same_name_is_rejected() {
        let (dir, library, track) = fixture();
        let root = dir.path().join("new");
        fs::create_dir_all(root.join("nested")).unwrap();
        library.register_folder(&root).unwrap();
        fs::write(root.join("song.wav"), b"different song").unwrap();
        let moved = root.join("nested/renamed.wav");
        fs::rename(&track.path, &moved).unwrap();
        assert_eq!(
            Path::new(&library.resolve_track_file(track.id).unwrap().path),
            moved.canonicalize().unwrap()
        );
    }

    #[test]
    fn same_name_with_different_content_stays_missing_without_changing_the_row() {
        let (dir, library, track) = fixture();
        let root = dir.path().join("new");
        fs::create_dir_all(&root).unwrap();
        library.register_folder(&root).unwrap();
        fs::write(root.join("song.wav"), b"different song").unwrap();
        fs::remove_file(&track.path).unwrap();
        assert!(matches!(
            library.resolve_track_file(track.id),
            Err(LibraryError::MissingFile(_))
        ));
        assert_eq!(library.get_track(track.id).unwrap().path, track.path);
        assert_eq!(library.cues(track.id).unwrap()[0].frame, 200);
    }

    #[test]
    fn unverified_tracks_require_an_explicit_file_selection() {
        let (dir, library, track) = fixture();
        let path = dir.path().join("pending.wav");
        fs::write(&path, b"original audio").unwrap();
        let id = library.register_local_files(&[path.clone()]).unwrap()[0];
        fs::remove_file(path).unwrap();
        assert!(matches!(
            library.resolve_track_file(id),
            Err(LibraryError::MissingFile(_))
        ));
        assert!(library.resolve_track_file(track.id).is_ok());
    }

    #[test]
    fn manual_replacement_invalidates_old_analysis_but_preserves_manual_cues_and_order() {
        let (dir, library, track) = fixture();
        let playlist = library.replace_playlist("Set", &[track.id]).unwrap();
        library
            .set_cue(track.id, 1, 50, CueKind::Hot, false)
            .unwrap();
        let replacement = dir.path().join("replacement.wav");
        fs::write(&replacement, b"replacement audio").unwrap();
        let hash = library.verified_content_hash(&replacement).unwrap();
        let linked = library
            .relink_track_file(&track, &replacement, &hash)
            .unwrap();
        assert_eq!(linked.id, track.id);
        assert_eq!(linked.content_hash, hash);
        assert!(!linked.analyzed);
        assert!(linked.bpm.is_none());
        assert_eq!(library.cues(track.id).unwrap().len(), 1);
        assert!(library.cues(track.id).unwrap()[0].user_set);
        assert_eq!(library.playlist_tracks(playlist).unwrap()[0].id, track.id);
        assert!(Path::new(&track.path).exists());
        assert!(matches!(
            library.relink_track_file(&track, &replacement, &hash),
            Err(LibraryError::SourceChanged)
        ));
    }

    #[test]
    fn changed_selection_and_duplicate_path_leave_both_track_identities_intact() {
        let (dir, library, track) = fixture();
        let other = dir.path().join("other.wav");
        fs::write(&other, b"other audio").unwrap();
        let other_id = library.import_file(&other).unwrap();
        let hash = library.verified_content_hash(&other).unwrap();
        assert!(matches!(
            library.relink_track_file(&track, &other, &hash),
            Err(LibraryError::SourceInUse)
        ));
        fs::write(&other, b"changed audio").unwrap();
        assert!(matches!(
            library.relink_track_file(&track, &other, &hash),
            Err(LibraryError::SourceChanged)
        ));
        assert_eq!(library.get_track(track.id).unwrap().path, track.path);
        assert!(library.get_track(other_id).is_ok());
    }

    #[test]
    fn removal_clears_all_membership_and_import_rows_without_deleting_audio() {
        let (_dir, library, track) = fixture();
        let playlist = library.replace_playlist("Set", &[track.id]).unwrap();
        library
            .save_import_items(
                playlist,
                &[ImportItem {
                    position: 0,
                    external_id: "remote".into(),
                    title: "Song".into(),
                    artist: "Artist".into(),
                    duration_ms: 1000,
                    status: "local".into(),
                    track_id: Some(track.id),
                    error: None,
                }],
            )
            .unwrap();
        library.remove_track(track.id).unwrap();
        assert!(library.list_tracks().unwrap().is_empty());
        assert!(library.playlist_tracks(playlist).unwrap().is_empty());
        assert!(library.import_items(playlist).unwrap().is_empty());
        assert!(library.cues(track.id).unwrap().is_empty());
        assert!(Path::new(&track.path).is_file());
    }
}
