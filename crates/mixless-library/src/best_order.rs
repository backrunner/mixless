//! Bounded, disposable Best order results. Keys are supplied by the planner
//! host and include its algorithm version, options and analysis fingerprints.
use super::*;

const MAX_ORDERS: usize = 128;

impl Library {
    pub fn load_best_order(
        &self,
        key: &str,
        count: usize,
    ) -> Result<Option<Vec<usize>>, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let payload: Option<String> = tx
            .query_row(
                "SELECT payload FROM best_orders WHERE cache_key=?1",
                [key],
                |r| r.get(0),
            )
            .optional()?;
        let result = payload
            .and_then(|s| serde_json::from_str::<Vec<usize>>(&s).ok())
            .filter(|order| permutation(order, count));
        if result.is_some() {
            tx.execute("UPDATE best_orders SET last_used=(SELECT COALESCE(MAX(last_used),0)+1 FROM best_orders) WHERE cache_key=?1", [key])?;
        } else {
            tx.execute("DELETE FROM best_orders WHERE cache_key=?1", [key])?;
        }
        tx.commit()?;
        Ok(result)
    }

    pub fn save_best_order(&self, key: &str, order: &[usize]) -> Result<(), LibraryError> {
        // Store occurrences, not track IDs: repeated songs must survive a hit.
        if !permutation(order, order.len()) {
            return Ok(());
        }
        let payload = serde_json::to_string(order)?;
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        tx.execute("INSERT INTO best_orders(cache_key,payload,last_used)
            VALUES (?1,?2,(SELECT COALESCE(MAX(last_used),0)+1 FROM best_orders))
            ON CONFLICT(cache_key) DO UPDATE SET payload=excluded.payload,last_used=excluded.last_used",
            params![key, payload])?;
        tx.execute(
            "DELETE FROM best_orders WHERE cache_key NOT IN
            (SELECT cache_key FROM best_orders ORDER BY last_used DESC LIMIT ?1)",
            [MAX_ORDERS],
        )?;
        tx.commit()?;
        Ok(())
    }
}

fn permutation(order: &[usize], count: usize) -> bool {
    if order.len() != count {
        return false;
    }
    let mut seen = vec![false; count];
    order
        .iter()
        .all(|&i| i < count && !std::mem::replace(&mut seen[i], true))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_library_migrates_additively() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let lib = Library::open(&path).unwrap();
        lib.external_playlist("spotify", "old", "Existing playlist")
            .unwrap();
        lib.conn.lock().unwrap().execute_batch("DROP TABLE best_orders; ALTER TABLE import_items DROP COLUMN album; PRAGMA user_version=2;").unwrap();
        drop(lib);
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.list_playlists().unwrap()[0].name, "Existing playlist");
        lib.save_best_order("migrated", &[2, 0, 1]).unwrap();
        assert_eq!(
            lib.load_best_order("migrated", 3).unwrap(),
            Some(vec![2, 0, 1])
        );
        assert!(lib.missing_spotify_albums().unwrap().is_empty());
    }

    #[test]
    fn reopens_multiple_playlists_and_preserves_occurrences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let lib = Library::open(&path).unwrap();
        lib.save_best_order("a", &[2, 0, 1]).unwrap();
        lib.save_best_order("b", &[1, 0, 3, 2]).unwrap();
        drop(lib);
        let lib = Library::open(&path).unwrap();
        assert_eq!(lib.load_best_order("a", 3).unwrap(), Some(vec![2, 0, 1]));
        assert_eq!(lib.load_best_order("b", 4).unwrap(), Some(vec![1, 0, 3, 2]));
        assert_eq!(lib.load_best_order("changed evidence", 3).unwrap(), None);
    }

    #[test]
    fn malformed_and_incompatible_entries_are_misses() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        for payload in ["broken", "[0,0,2]", "[0,1,9]", "[0,1]", "[-1,0,1]"] {
            lib.conn
                .lock()
                .unwrap()
                .execute(
                    "INSERT OR REPLACE INTO best_orders VALUES ('bad',?1,0)",
                    [payload],
                )
                .unwrap();
            assert_eq!(lib.load_best_order("bad", 3).unwrap(), None);
        }
        lib.save_best_order("invalid", &[0, 0, 2]).unwrap();
        assert_eq!(lib.load_best_order("invalid", 3).unwrap(), None);
    }

    #[test]
    fn retention_keeps_recently_used_playlists() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        for i in 0..MAX_ORDERS {
            lib.save_best_order(&i.to_string(), &[0, 1, 2]).unwrap();
        }
        assert!(lib.load_best_order("0", 3).unwrap().is_some());
        lib.save_best_order("new", &[0, 1, 2]).unwrap();
        assert!(lib.load_best_order("1", 3).unwrap().is_none());
        assert!(lib.load_best_order("0", 3).unwrap().is_some());
    }
}
