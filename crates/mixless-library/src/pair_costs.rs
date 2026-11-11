//! Disposable, bounded cold-pair costs; exact f32 bits survive reopening.
use super::*;
const MAX_PAIRS: usize = 20_000;

impl Library {
    pub fn load_pair_costs(&self, keys: &[String]) -> Result<Vec<Option<f32>>, LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let tick: i64 = tx.query_row(
            "SELECT COALESCE(MAX(last_used),0) FROM pair_costs",
            [],
            |r| r.get(0),
        )?;
        let mut values = Vec::with_capacity(keys.len());
        {
            let mut get = tx.prepare("SELECT cost_bits FROM pair_costs WHERE cache_key=?1")?;
            let mut touch = tx.prepare("UPDATE pair_costs SET last_used=?2 WHERE cache_key=?1")?;
            let mut discard = tx.prepare("DELETE FROM pair_costs WHERE cache_key=?1")?;
            for (i, key) in keys.iter().enumerate() {
                let raw: Option<rusqlite::types::Value> =
                    get.query_row([key], |r| r.get(0)).optional()?;
                let value = raw
                    .as_ref()
                    .and_then(|v| match v {
                        rusqlite::types::Value::Integer(bits) => {
                            u32::try_from(*bits).ok().map(f32::from_bits)
                        }
                        _ => None,
                    })
                    .filter(|v| v.is_finite() && *v >= 0.);
                if value.is_some() {
                    touch.execute(params![key, tick + i as i64 + 1])?;
                } else if raw.is_some() {
                    discard.execute([key])?;
                }
                values.push(value);
            }
        }
        tx.commit()?;
        Ok(values)
    }

    pub fn save_pair_costs(&self, entries: &[(String, f32)]) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let tick: i64 = tx.query_row(
            "SELECT COALESCE(MAX(last_used),0) FROM pair_costs",
            [],
            |r| r.get(0),
        )?;
        {
            let mut insert = tx.prepare("INSERT INTO pair_costs(cache_key,cost_bits,last_used) VALUES (?1,?2,?3) ON CONFLICT(cache_key) DO UPDATE SET cost_bits=excluded.cost_bits,last_used=excluded.last_used")?;
            for (i, (key, cost)) in entries
                .iter()
                .enumerate()
                .filter(|(_, (_, c))| c.is_finite() && *c >= 0.)
            {
                insert.execute(params![key, cost.to_bits() as i64, tick + i as i64 + 1])?;
            }
        }
        tx.execute("DELETE FROM pair_costs WHERE cache_key NOT IN (SELECT cache_key FROM pair_costs ORDER BY last_used DESC LIMIT ?1)", [MAX_PAIRS as i64])?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_reopening_and_invalid_values_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.db");
        let lib = Library::open(&path).unwrap();
        lib.conn
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE pair_costs; PRAGMA user_version=4;")
            .unwrap();
        drop(lib);
        let lib = Library::open(&path).unwrap();
        let cost = f32::from_bits(0x3f123456);
        lib.save_pair_costs(&[
            ("valid".into(), cost),
            ("nan".into(), f32::NAN),
            ("negative".into(), -1.),
        ])
        .unwrap();
        lib.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO pair_costs(cache_key,cost_bits,last_used) VALUES ('bad',?1,0)",
                [f32::INFINITY.to_bits() as i64],
            )
            .unwrap();
        drop(lib);
        let lib = Library::open(&path).unwrap();
        let keys = ["valid", "nan", "negative", "bad", "missing"].map(str::to_owned);
        let values = lib.load_pair_costs(&keys).unwrap();
        assert_eq!(values[0].unwrap().to_bits(), cost.to_bits());
        assert!(values[1..].iter().all(Option::is_none));
        assert_eq!(
            lib.conn
                .lock()
                .unwrap()
                .query_row("SELECT count(*) FROM pair_costs", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    #[test]
    fn retention_preserves_recently_read_pairs() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let entries: Vec<_> = (0..MAX_PAIRS).map(|i| (i.to_string(), i as f32)).collect();
        lib.save_pair_costs(&entries).unwrap();
        lib.load_pair_costs(&["0".into()]).unwrap();
        lib.save_pair_costs(&[("new".into(), 1.)]).unwrap();
        assert_eq!(
            lib.load_pair_costs(&["0".into(), "1".into(), "new".into()])
                .unwrap(),
            vec![Some(0.), None, Some(1.)]
        );
    }
}
