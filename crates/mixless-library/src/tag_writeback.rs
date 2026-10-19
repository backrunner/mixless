//! Atomic tag writeback preserves audio identity and the analyses derived from it.
use super::*;
use lofty::{config::WriteOptions, tag::Tag};

impl Library {
    /// Runs on an import/analysis worker. Unsupported/read-only files return an
    /// error without changing the source; callers keep the library metadata.
    pub fn write_track_metadata(&self, id: TrackId) -> Result<bool, LibraryError> {
        let track = self.get_track(id)?;
        let path = Path::new(&track.path);
        let before = self.verified_content_hash(path)?;
        if before != track.content_hash {
            return Err(LibraryError::SourceChanged);
        }
        let mut tagged = Probe::open(path)?.read()?;
        let tag_type = tagged.primary_tag_type();
        if tagged.primary_tag().is_none() {
            tagged.insert_tag(Tag::new(tag_type));
        }
        let tag = tagged
            .primary_tag_mut()
            .ok_or_else(|| std::io::Error::other("Audio has no writable tags"))?;
        let mut changed = false;
        let mut text = |key, value: Option<String>| {
            if let Some(value) = value.filter(|s| !s.trim().is_empty()) {
                if tag.get_string(&key) != Some(value.as_str()) {
                    tag.insert_text(key, value);
                    changed = true;
                }
            }
        };
        // Never manufacture tags from a fallback filename or Unknown artist.
        let fallback = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        text(
            ItemKey::TrackTitle,
            (track.title != fallback).then(|| track.title.clone()),
        );
        text(
            ItemKey::TrackArtist,
            (track.artist != "Unknown").then(|| track.artist.clone()),
        );
        text(ItemKey::AlbumTitle, track.album.clone());
        text(ItemKey::Isrc, track.isrc.clone());
        if matches!(
            tag_type,
            lofty::tag::TagType::Id3v2 | lofty::tag::TagType::Mp4Ilst
        ) {
            text(
                ItemKey::IntegerBpm,
                track
                    .bpm
                    .filter(|b| b.is_finite() && *b > 0.)
                    .map(|b| format!("{b:.0}")),
            );
        }
        if tag_type != lofty::tag::TagType::Id3v2 {
            text(
                ItemKey::Bpm,
                track
                    .bpm
                    .filter(|b| b.is_finite() && *b > 0.)
                    .map(|b| format!("{b:.2}")),
            );
        }
        text(
            ItemKey::InitialKey,
            track.key.as_deref().map(|key| {
                if let Some(root) = key.strip_suffix(" major") {
                    root.to_owned()
                } else if let Some(root) = key.strip_suffix(" minor") {
                    format!("{root}m")
                } else {
                    key.to_owned()
                }
            }),
        );
        if !changed {
            return Ok(false);
        }
        if fs::metadata(path)?.permissions().readonly() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Audio file is read-only",
            )
            .into());
        }
        let audio_hash = self.stem_hash(path)?;
        // Keep existing container fields, pictures, and unrelated DJ tags.
        let staged = tempfile::Builder::new()
            .prefix(".mixless-tags-")
            .suffix(&format!(
                ".{}",
                path.extension().and_then(|s| s.to_str()).unwrap_or("audio")
            ))
            .tempfile_in(path.parent().unwrap_or(Path::new(".")))?;
        fs::copy(path, staged.path())?;
        tagged.save_to_path(staged.path(), WriteOptions::default())?;
        fs::File::open(staged.path())?.sync_all()?;
        if stem_content_hash(staged.path())? != audio_hash {
            return Err(std::io::Error::other("Tag writer changed the audio stream").into());
        }
        let after = content_hash(staged.path())?;
        let mut conn = self.conn.lock().expect("library mutex");
        let tx = conn.transaction()?;
        let current: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM tracks WHERE id=?1 AND path=?2 AND content_hash=?3 AND title=?4 AND artist=?5 AND album IS ?6 AND isrc IS ?7 AND bpm IS ?8 AND key IS ?9)",params![id.0,track.path,before,track.title,track.artist,track.album,track.isrc,track.bpm,track.key],|r|r.get(0))?;
        if !current || content_hash(path)? != before {
            return Err(LibraryError::SourceChanged);
        }
        fs::rename(staged.path(), path)?;
        tx.execute(
            "UPDATE tracks SET content_hash=?2 WHERE id=?1",
            params![id.0, after],
        )?;
        for table in ["track_analysis", "track_waveforms"] {
            tx.execute(
                &format!(
                    "UPDATE {table} SET content_hash=?2 WHERE track_id=?1 AND content_hash=?3"
                ),
                params![id.0, after, before],
            )?;
        }
        tx.execute(
            "INSERT OR REPLACE INTO stem_identities(file_hash,audio_hash) VALUES (?1,?2)",
            params![after, audio_hash],
        )?;
        // Preserve the new file revision/size hint so later rename recovery
        // can find these exact bytes even before another playback verification.
        if let Some(fingerprint) = crate::verification::fingerprint(path)? {
            tx.execute("INSERT OR REPLACE INTO file_verification(path,fingerprint,content_hash) VALUES (?1,?2,?3)",params![track.path,fingerprint,after])?;
        } else {
            tx.execute("DELETE FROM file_verification WHERE path=?1", [track.path])?;
        }
        tx.commit()?;
        Ok(true)
    }

    pub fn write_track_metadata_if_possible(&self, id: TrackId) {
        if let Err(error) = self.write_track_metadata(id) {
            tracing::warn!(?id,%error,"Could not write audio metadata");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn writes_tags_atomically_preserves_audio_cues_analysis_and_unrelated_tags() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.wav");
        let mut wav = hound::WavWriter::create(
            &path,
            hound::WavSpec {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for i in 0..8820 {
            wav.write_sample::<i16>((i % 300) as i16).unwrap();
        }
        wav.finalize().unwrap();
        use lofty::prelude::TagExt;
        let mut tag = Tag::new(lofty::tag::TagType::Id3v2);
        tag.insert_text(ItemKey::Comment, "Keep this DJ note".into());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let id = lib.import_file(&path).unwrap();
        let audio = lib.stem_hash(&path).unwrap();
        lib.set_recording_metadata(id, "Known song", "Known artist", Some("USABC1234567"))
            .unwrap();
        lib.set_analysis_meta(id, Some(128.), Some("Am"), Some("8A"))
            .unwrap();
        lib.set_cue(id, 0, 1200, CueKind::Hot, true).unwrap();
        let hash = lib.get_track(id).unwrap().content_hash;
        lib.conn.lock().unwrap().execute("INSERT INTO track_analysis(track_id,content_hash,version,payload) VALUES (?1,?2,22,'{}')",params![id.0,hash]).unwrap();
        assert!(lib.write_track_metadata(id).unwrap());
        assert_eq!(audio, lib.stem_hash(&path).unwrap());
        let track = lib.get_track(id).unwrap();
        assert_ne!(hash, track.content_hash);
        assert_eq!(track.content_hash, content_hash(&path).unwrap());
        let saved: String = lib
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT content_hash FROM track_analysis WHERE track_id=?1",
                [id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(saved, track.content_hash);
        let tagged = Probe::open(&path).unwrap().read().unwrap();
        let tag = tagged.primary_tag().unwrap();
        assert_eq!(tag.get_string(&ItemKey::Comment), Some("Keep this DJ note"));
        assert_eq!(tag.title().as_deref(), Some("Known song"));
        assert_eq!(tag.artist().as_deref(), Some("Known artist"));
        assert_eq!(tag.get_string(&ItemKey::InitialKey), Some("Am"));
        assert_eq!(tag.get_string(&ItemKey::IntegerBpm), Some("128"));
        assert_eq!(lib.cues(id).unwrap()[0].frame, 1200);
        assert!(!lib.write_track_metadata(id).unwrap());
        fs::write(&path, b"different audio").unwrap();
        assert!(matches!(
            lib.write_track_metadata(id),
            Err(LibraryError::SourceChanged)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"different audio");
    }
}
