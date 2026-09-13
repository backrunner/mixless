//! Atomic manual ordering, independent for each playlist and All Tracks.
use super::*;

impl Library {
    /// Save only a permutation of the exact list the user dragged. A concurrent
    /// import/removal must never be silently overwritten by an older UI snapshot.
    pub fn reorder_tracks(
        &self,
        playlist: Option<PlaylistId>,
        expected: &[TrackId],
        ordered: &[TrackId],
    ) -> Result<(), LibraryError> {
        let mut before: Vec<_> = expected.iter().map(|id| id.0).collect();
        let mut after: Vec<_> = ordered.iter().map(|id| id.0).collect();
        before.sort_unstable();
        after.sort_unstable();
        if before != after {
            return Err(LibraryError::OrderChanged);
        }
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let current = if let Some(playlist) = playlist {
            let mut query = tx.prepare(
                "SELECT track_id FROM playlist_items WHERE playlist_id=?1 ORDER BY position",
            )?;
            let rows = query.query_map([playlist.0], |row| Ok(TrackId(row.get(0)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        } else {
            let mut query = tx.prepare(
                "SELECT t.id FROM tracks t LEFT JOIN library_order o ON o.track_id=t.id
                 ORDER BY o.position IS NULL, o.position, t.artist, t.title, t.id",
            )?;
            let rows = query.query_map([], |row| Ok(TrackId(row.get(0)?)))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        if current != expected {
            return Err(LibraryError::OrderChanged);
        }
        if current == ordered {
            return Ok(());
        }
        if let Some(playlist) = playlist {
            tx.execute(
                "DELETE FROM playlist_items WHERE playlist_id=?1",
                [playlist.0],
            )?;
            let mut insert = tx.prepare(
                "INSERT INTO playlist_items(playlist_id,position,track_id) VALUES (?1,?2,?3)",
            )?;
            for (position, id) in ordered.iter().enumerate() {
                insert.execute(params![playlist.0, position as i64, id.0])?;
            }
        } else {
            tx.execute("DELETE FROM library_order", [])?;
            let mut insert =
                tx.prepare("INSERT INTO library_order(track_id,position) VALUES (?1,?2)")?;
            for (position, id) in ordered.iter().enumerate() {
                insert.execute(params![id.0, position as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

/// Import refreshes change membership, never the relative order of songs the
/// listener has already arranged. Counts preserve repeated playlist entries.
pub(super) fn refreshed_order(
    tx: &rusqlite::Transaction<'_>,
    playlist: PlaylistId,
    incoming: &[TrackId],
) -> Result<Vec<TrackId>, LibraryError> {
    let mut query =
        tx.prepare("SELECT track_id FROM playlist_items WHERE playlist_id=?1 ORDER BY position")?;
    let old = query
        .query_map([playlist.0], |r| Ok(TrackId(r.get(0)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let mut remaining = std::collections::HashMap::<TrackId, usize>::new();
    for id in incoming {
        *remaining.entry(*id).or_default() += 1;
    }
    let mut ordered = Vec::with_capacity(incoming.len());
    for id in old.iter().chain(incoming) {
        if let Some(count) = remaining.get_mut(id) {
            if *count > 0 {
                ordered.push(*id);
                *count -= 1;
            }
        }
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Library, Vec<TrackId>) {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let ids = ["A", "B", "C", "D"].map(|name| {
            let path = dir.path().join(format!("{name}.wav"));
            fs::write(&path, name).unwrap();
            lib.register_local_files(&[path]).unwrap()[0]
        });
        (dir, lib, ids.to_vec())
    }

    fn ids(tracks: Vec<Track>) -> Vec<TrackId> {
        tracks.into_iter().map(|track| track.id).collect()
    }

    #[test]
    fn refreshed_import_keeps_manual_order_duplicates_and_appends_new_tracks() {
        let (dir, lib, t) = fixture();
        let playlist = lib
            .replace_playlist("Set", &[t[0], t[1], t[0], t[2]])
            .unwrap();
        let manual = [t[2], t[0], t[0], t[1]];
        lib.reorder_tracks(Some(playlist), &[t[0], t[1], t[0], t[2]], &manual)
            .unwrap();
        let ready = [t[0], t[1], t[2], t[3], t[0]];
        let items: Vec<_> = ready
            .iter()
            .enumerate()
            .map(|(position, id)| crate::ImportItem {
                position,
                external_id: format!("song-{}", id.0),
                title: "song".into(),
                artist: "artist".into(),
                duration_ms: 1000,
                status: "local".into(),
                track_id: Some(*id),
                error: None,
            })
            .collect();
        lib.save_import_items(playlist, &items).unwrap();
        assert_eq!(
            ids(lib.playlist_tracks(playlist).unwrap()),
            [t[2], t[0], t[0], t[1], t[3]]
        );
        lib.replace_playlist("Set", &[t[0], t[1], t[3]]).unwrap();
        drop(lib);
        let reopened = Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(
            ids(reopened.playlist_tracks(playlist).unwrap()),
            [t[0], t[1], t[3]]
        );
    }

    #[test]
    fn orders_survive_reopen_and_remain_independent() {
        let (dir, lib, original) = fixture();
        let playlist = lib.replace_playlist("Set", &original).unwrap();
        let other = lib.replace_playlist("Other", &original).unwrap();
        let global = ids(lib.list_tracks().unwrap());
        let reversed: Vec<_> = global.iter().copied().rev().collect();
        lib.reorder_tracks(None, &global, &reversed).unwrap();
        let set_order = [original[2], original[0], original[3], original[1]];
        lib.reorder_tracks(Some(playlist), &original, &set_order)
            .unwrap();
        drop(lib);
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        assert_eq!(ids(lib.playlist_tracks(playlist).unwrap()), set_order);
        assert_eq!(ids(lib.playlist_tracks(other).unwrap()), original);
        assert_eq!(ids(lib.list_tracks().unwrap()), reversed);
        assert!(lib
            .list_tracks()
            .unwrap()
            .iter()
            .all(|t| Path::new(&t.path).exists()));
    }

    #[test]
    fn duplicates_and_position_gaps_are_preserved_correctly() {
        let (_dir, lib, tracks) = fixture();
        let playlist = lib
            .replace_playlist("Set", &[tracks[0], tracks[1], tracks[0], tracks[2]])
            .unwrap();
        lib.remove_playlist_track(playlist, tracks[1]).unwrap();
        let expected = [tracks[0], tracks[0], tracks[2]];
        let ordered = [tracks[0], tracks[2], tracks[0]];
        lib.reorder_tracks(Some(playlist), &expected, &ordered)
            .unwrap();
        assert_eq!(ids(lib.playlist_tracks(playlist).unwrap()), ordered);
    }

    #[test]
    fn stale_or_invalid_orders_do_not_change_membership() {
        let (_dir, lib, original) = fixture();
        let playlist = lib.replace_playlist("Set", &original).unwrap();
        let reversed: Vec<_> = original.iter().copied().rev().collect();
        assert!(lib
            .reorder_tracks(Some(playlist), &original, &original[..2])
            .is_err());
        lib.remove_playlist_track(playlist, original[0]).unwrap();
        assert!(lib
            .reorder_tracks(Some(playlist), &original, &reversed)
            .is_err());
        assert_eq!(ids(lib.playlist_tracks(playlist).unwrap()), original[1..]);
    }

    #[test]
    fn folder_backfill_and_new_imports_keep_manual_order() {
        let (dir, lib, original) = fixture();
        let folder = PlaylistId(lib.list_playlists().unwrap()[0].id);
        let expected = ids(lib.playlist_tracks(folder).unwrap());
        let reversed: Vec<_> = expected.iter().copied().rev().collect();
        lib.reorder_tracks(Some(folder), &expected, &reversed)
            .unwrap();
        let global = ids(lib.list_tracks().unwrap());
        lib.reorder_tracks(None, &global, &reversed).unwrap();
        let path = dir.path().join("New.wav");
        fs::write(&path, b"new track").unwrap();
        let new = lib.register_local_files(&[path]).unwrap()[0];
        drop(lib);
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let mut ordered = reversed;
        ordered.push(new);
        assert_eq!(ids(lib.playlist_tracks(folder).unwrap()), ordered);
        assert_eq!(ids(lib.list_tracks().unwrap()), ordered);
        assert_eq!(ordered.len(), original.len() + 1);
    }
}
