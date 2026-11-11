//! Source integrity uses full-file hashes; derived stems use encoded audio identity.
use super::*;
use symphonia::core::{
    errors::Error as AudioError,
    formats::FormatOptions,
    formats::{probe::Hint, TrackType},
    io::MediaSourceStream,
    meta::MetadataOptions,
};

/// Hash decoder configuration and every audio packet, excluding container tags.
/// Unsupported or damaged containers retain strict full-file identity.
pub fn stem_content_hash(path: &Path) -> Result<String, LibraryError> {
    match audio_hash(path) {
        Ok(hash) => Ok(hash),
        Err(_) => content_hash(path),
    }
}
fn audio_hash(path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    let stream = MediaSourceStream::new(Box::new(fs::File::open(path)?), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
        hint.with_extension(ext);
    }
    let mut reader = symphonia::default::get_probe().probe(
        &hint,
        stream,
        FormatOptions::default(),
        MetadataOptions::default(),
    )?;
    let track = reader
        .default_track(TrackType::Audio)
        .ok_or("No audio stream")?;
    let id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or("No audio codec")?;
    let mut hash = blake3::Hasher::new();
    // Versioned binary metadata, independent of upstream Debug formatting.
    hash.update(b"mixless-audio-v2\0");
    let codec = u32::from_str_radix(params.codec.to_string().trim_start_matches("0x"), 16)?;
    hash.update(&codec.to_le_bytes());
    for value in [
        params.sample_rate,
        params.bits_per_sample,
        track.delay,
        track.padding,
    ] {
        hash.update(&[u8::from(value.is_some())]);
        hash.update(&value.unwrap_or_default().to_le_bytes());
    }
    use symphonia::core::audio::Channels;
    let (kind, channels) = match &params.channels {
        None => (0, 0),
        Some(Channels::None) => (1, 0),
        Some(Channels::Positioned(positions)) => (2, positions.bits()),
        Some(Channels::Discrete(count)) => (3, u64::from(*count)),
        Some(Channels::Ambisonic(order)) => (4, u64::from(*order)),
        _ => return Err("Unsupported custom channel identity".into()),
    };
    hash.update(&[kind]);
    hash.update(&channels.to_le_bytes());
    if let Some(extra) = &params.extra_data {
        hash.update(&(extra.len() as u64).to_le_bytes());
        hash.update(extra);
    }
    let mut packets = 0u64;
    loop {
        match reader.next_packet() {
            Ok(Some(packet)) if packet.track_id == id => {
                hash.update(&(packet.data.len() as u64).to_le_bytes());
                hash.update(&packet.data);
                packets += 1;
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(AudioError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(e.into()),
        }
    }
    if packets == 0 {
        return Err("No audio packets".into());
    }
    Ok(format!("audio-v2-{}", hash.finalize().to_hex()))
}
impl Library {
    /// Persistent memoization is addressed by verified full-file bytes, never by filename.
    pub fn stem_hash(&self, path: &Path) -> Result<String, LibraryError> {
        let file_hash = self.verified_content_hash(path)?;
        if let Some(hash) = self
            .conn
            .lock()
            .expect("library mutex")
            .query_row(
                "SELECT audio_hash FROM stem_identities WHERE file_hash=?1",
                [&file_hash],
                |r| r.get(0),
            )
            .optional()?
        {
            return Ok(hash);
        }
        let hash = stem_content_hash(path)?;
        if self.verified_content_hash(path)? != file_hash {
            return Err(LibraryError::SourceChanged);
        }
        self.conn.lock().expect("library mutex").execute(
            "INSERT OR REPLACE INTO stem_identities(file_hash,audio_hash) VALUES (?1,?2)",
            params![file_hash, hash],
        )?;
        Ok(hash)
    }

    pub fn stem_cache_keys(
        &self,
        playlist: Option<PlaylistId>,
    ) -> Result<Vec<(TrackId, String)>, LibraryError> {
        let ids = self.cache_keys(playlist)?;
        ids.into_iter()
            .map(|(id, fallback)| {
                let track = self.get_track(id)?;
                // Offline/removable sources can still be accounted for and cleared.
                let cached = self
                    .conn
                    .lock()
                    .expect("library mutex")
                    .query_row(
                        "SELECT audio_hash FROM stem_identities WHERE file_hash=?1",
                        [&fallback],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?;
                let hash = match cached {
                    Some(hash) => hash,
                    None if Path::new(&track.path).is_file() => {
                        self.stem_hash(Path::new(&track.path))?
                    }
                    None => fallback,
                };
                Ok((id, hash))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lofty::{
        config::WriteOptions,
        prelude::TagExt,
        tag::{Tag, TagType},
    };
    #[test]
    fn tags_and_renames_reuse_stems_but_audio_edits_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("first.wav");
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
        for i in 0..4410 {
            wav.write_sample::<i16>((i % 300) as i16).unwrap();
        }
        wav.finalize().unwrap();
        let lib = Library::open(&dir.path().join("library.db")).unwrap();
        let before = lib.stem_hash(&path).unwrap();
        assert!(before.starts_with("audio-v2-"));
        let bytes = content_hash(&path).unwrap();
        let mut tag = Tag::new(TagType::RiffInfo);
        tag.set_title("Song".into());
        tag.set_artist("Artist".into());
        tag.save_to_path(&path, WriteOptions::default()).unwrap();
        assert_ne!(bytes, content_hash(&path).unwrap());
        assert_eq!(before, lib.stem_hash(&path).unwrap());
        let renamed = dir.path().join("Artist - Song.wav");
        fs::rename(&path, &renamed).unwrap();
        assert_eq!(before, lib.stem_hash(&renamed).unwrap());
        let mut data = fs::read(&renamed).unwrap();
        let data_start = data.windows(4).position(|w| w == b"data").unwrap() + 8;
        data[data_start + 100] ^= 1;
        fs::write(&renamed, data).unwrap();
        assert_ne!(before, lib.stem_hash(&renamed).unwrap());
    }
}
