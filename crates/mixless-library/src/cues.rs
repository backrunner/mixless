use super::*;
impl Library {
    /// Read only the source clock needed to position cached cue markers.
    pub fn cue_sample_rate(&self, id: TrackId) -> Result<Option<u32>, LibraryError> {
        Ok(self.conn.lock().expect("library mutex").query_row(
            "SELECT json_extract(a.payload, '$.sample_rate') FROM track_analysis a JOIN tracks t ON t.id=a.track_id WHERE a.track_id=?1 AND a.content_hash=t.content_hash",
            params![id.0], |r|r.get(0)).optional()?)
    }

    /// Replace only generated slots, in one transaction. User edits survive
    /// reanalysis and cannot be overwritten by an in-flight analysis worker.
    pub fn replace_auto_cues(&self, id: TrackId, cues: &[Cue]) -> Result<(), LibraryError> {
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM cues WHERE track_id=?1 AND user_set=0",
            params![id.0],
        )?;
        for cue in cues.iter().filter(|c| c.index < 8 && !c.user_set) {
            let kind = match cue.kind {
                CueKind::In => "in",
                CueKind::Out => "out",
                CueKind::Hot => "hot",
            };
            tx.execute("INSERT OR IGNORE INTO cues (track_id,idx,frame,kind,user_set) VALUES (?1,?2,?3,?4,0)",params![id.0,cue.index as i64,cue.frame as i64,kind])?;
        }
        tx.execute("INSERT INTO cue_versions(track_id,version) VALUES (?1,1) ON CONFLICT(track_id) DO UPDATE SET version=1", [id.0])?;
        tx.commit()?;
        Ok(())
    }
    pub fn automatic_cues_current(&self, id: TrackId) -> Result<bool, LibraryError> {
        Ok(self
            .conn
            .lock()
            .expect("library mutex")
            .query_row(
                "SELECT version=1 FROM cue_versions WHERE track_id=?1",
                [id.0],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }
    pub fn clear_cue(&self, id: TrackId, index: u8) -> Result<(), LibraryError> {
        self.conn.lock().expect("library mutex").execute(
            "DELETE FROM cues WHERE track_id=?1 AND idx=?2",
            params![id.0, index as i64],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn automatic_refresh_never_overwrites_manual_hot_or_mix_cues() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("track.wav");
        std::fs::write(&path, b"fixture").unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let id = lib.import_file(&path).unwrap();
        lib.set_cue(id, 0, 200, CueKind::In, true).unwrap();
        let auto = vec![
            Cue {
                index: 0,
                frame: 0,
                kind: CueKind::Hot,
                user_set: false,
            },
            Cue {
                index: 1,
                frame: 100,
                kind: CueKind::Out,
                user_set: false,
            },
        ];
        lib.replace_auto_cues(id, &auto).unwrap();
        lib.replace_auto_cues(id, &auto).unwrap();
        let cues = lib.cues(id).unwrap();
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].frame, 200);
        assert!(cues[0].user_set);
        lib.clear_cue(id, 0).unwrap();
        assert_eq!(lib.cues(id).unwrap().len(), 1);
    }
}
