//! Persist recording identity in the audio so other DJ software sees it too.
use super::*;
use lofty::{
    config::WriteOptions,
    file::{AudioFile, TaggedFileExt},
    prelude::Accessor,
    probe::Probe,
    tag::{ItemKey, Tag, TagType},
};

pub(super) fn write(path: &Path, job: &ResolveJob) -> Result<(), AcquireError> {
    let mut file = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|e| err(format!("Read audio tags: {e}")))?;
    if file.primary_tag().is_none() {
        file.insert_tag(Tag::new(TagType::Mp4Ilst));
    }
    let tag = file
        .primary_tag_mut()
        .ok_or_else(|| err("Audio has no writable metadata"))?;
    tag.set_title(job.title.clone());
    tag.set_artist(job.artist.clone());
    if let Some(isrc) = &job.isrc {
        tag.insert_text(ItemKey::Isrc, isrc.clone());
    }
    file.save_to_path(path, WriteOptions::default())
        .map_err(|e| err(format!("Write audio tags: {e}")))
}
