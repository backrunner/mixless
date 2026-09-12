//! Compact, versioned spectral envelopes. PCM never enters the database.
use super::*;
use mixless_protocol::Waveform;

pub const WAVEFORM_VERSION: u32 = 2;
impl Library {
    pub fn save_waveform(
        &self,
        id: TrackId,
        hash: &str,
        wave: &Waveform,
    ) -> Result<(), LibraryError> {
        let mut payload = Vec::with_capacity(wave.columns as usize * 14);
        for band in [
            &wave.peak,
            &wave.peak_pos,
            &wave.peak_neg,
            &wave.rms,
            &wave.low,
            &wave.low_mid,
            &wave.mid,
            &wave.high,
        ] {
            if band.len() != wave.columns as usize {
                return Err(std::io::Error::other("Invalid waveform extent").into());
            }
            payload.extend_from_slice(band);
        }
        for band in [&wave.detail_pos, &wave.detail_neg, &wave.detail_rms] {
            if band.len() != wave.columns as usize {
                return Err(std::io::Error::other("Invalid detailed waveform extent").into());
            }
            for value in band {
                payload.extend_from_slice(&value.to_le_bytes());
            }
        }
        self.conn.lock().expect("library mutex").execute(
            "INSERT INTO track_waveforms(track_id,content_hash,version,duration,payload)
             SELECT id,content_hash,?3,?4,?5 FROM tracks WHERE id=?1 AND content_hash=?2
             ON CONFLICT(track_id) DO UPDATE SET content_hash=excluded.content_hash,
             version=excluded.version,duration=excluded.duration,payload=excluded.payload",
            params![id.0, hash, WAVEFORM_VERSION, wave.duration_sec, payload],
        )?;
        Ok(())
    }

    pub fn load_waveform(&self, id: TrackId) -> Result<Option<Waveform>, LibraryError> {
        let row: Option<(f32, Vec<u8>)> = self.conn.lock().expect("library mutex").query_row(
            "SELECT w.duration,w.payload FROM track_waveforms w JOIN tracks t ON w.track_id=t.id
             WHERE w.track_id=?1 AND w.content_hash=t.content_hash AND w.version=?2",
            params![id.0,WAVEFORM_VERSION], |row| Ok((row.get(0)?,row.get(1)?)),
        ).optional()?;
        Ok(row.and_then(|(duration_sec, payload)| {
            if payload.is_empty()
                || payload.len() % 14 != 0
                || !duration_sec.is_finite()
                || duration_sec <= 0.
            {
                return None;
            }
            let n = payload.len() / 14;
            let mut parts = payload[..n * 8].chunks_exact(n).map(|part| part.to_vec());
            Some(Waveform {
                columns: n as u32,
                duration_sec,
                peak: parts.next()?,
                peak_pos: parts.next()?,
                peak_neg: parts.next()?,
                rms: parts.next()?,
                low: parts.next()?,
                low_mid: parts.next()?,
                mid: parts.next()?,
                high: parts.next()?,
                detail_pos: payload[n * 8..n * 10]
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect(),
                detail_neg: payload[n * 10..n * 12]
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect(),
                detail_rms: payload[n * 12..n * 14]
                    .chunks_exact(2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]))
                    .collect(),
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waveform_survives_restart_and_content_edits_invalidate_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("song.wav");
        fs::write(&path, b"original").unwrap();
        let db = dir.path().join("library.db");
        let lib = Library::open(&db).unwrap();
        let id = lib.import_file(&path).unwrap();
        let wave = Waveform {
            columns: 2,
            duration_sec: 4.,
            peak: vec![1, 2],
            peak_pos: vec![1, 2],
            peak_neg: vec![2, 1],
            detail_pos: vec![257, 514],
            detail_neg: vec![514, 257],
            detail_rms: vec![257, 257],
            low_mid: vec![0; 2],
            rms: vec![1, 1],
            low: vec![255, 0],
            mid: vec![0, 255],
            high: vec![0, 0],
        };
        lib.save_waveform(id, &lib.get_track(id).unwrap().content_hash, &wave)
            .unwrap();
        drop(lib);
        let lib = Library::open(&db).unwrap();
        let cached = lib.load_waveform(id).unwrap().unwrap();
        assert_eq!(cached.low, wave.low);
        assert_eq!(cached.low_mid, wave.low_mid);
        assert_eq!(cached.detail_pos, wave.detail_pos);
        assert_eq!(cached.detail_neg, wave.detail_neg);
        assert_eq!(cached.detail_rms, wave.detail_rms);
        fs::write(&path, b"edited").unwrap();
        lib.import_file(&path).unwrap();
        assert!(lib.load_waveform(id).unwrap().is_none());
    }
    #[test]
    fn discovery_lists_all_files_before_analysis_including_invalid_audio() {
        let dir = tempfile::tempdir().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let folder = dir.path().join("Set");
        fs::create_dir(&folder).unwrap();
        let playlist = lib.register_folder(&folder).unwrap();
        assert_eq!(lib.list_playlists().unwrap()[0].tracks, 0);
        let paths = [folder.join("first.wav"), folder.join("broken.mp3")];
        for path in &paths {
            fs::write(path, b"pending").unwrap();
        }
        let ids = lib.register_local_files(&paths).unwrap();
        let rows = lib.playlist_tracks(playlist).unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|track| !track.analyzed));
        assert_eq!(lib.register_local_files(&paths).unwrap(), ids);
        assert_eq!(lib.playlist_tracks(playlist).unwrap().len(), 2);
    }
}
