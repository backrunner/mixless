use super::*;

/// Verify and stage every referenced object before publishing library rows.
/// Audio is restored into app-managed storage; the package may then be removed.
pub fn import(
    library: &Library,
    stems: &Processor,
    path: &Path,
    mut progress: impl FnMut(String),
    active: impl Fn() -> bool,
) -> Result<Report> {
    check(&active)?;
    let mut pack = Container::open(path, false)?;
    let sources = std::mem::take(&mut pack.manifest.sources);
    let mut report = Report::default();
    let root = library.package_media_dir();
    fs::create_dir_all(&root)?;
    let _lock = {
        let lock = File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".import.lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)?;
        lock
    };
    let staging = tempfile::Builder::new()
        .prefix(".import-")
        .tempdir_in(&root)?;
    let mut staged = Vec::new();
    // No filesystem path in the manifest is ever used as an extraction path.
    for (source_id, source) in &sources {
        if source_id.len() != 32 || !source_id.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::Invalid("Invalid library identity".into()));
        }
        if source.order.len() != source.tracks.len()
            || source.order.iter().collect::<HashSet<_>>().len() != source.order.len()
            || source
                .order
                .iter()
                .any(|id| !source.tracks.contains_key(id))
        {
            return Err(Error::Invalid("Invalid library order".into()));
        }
        for playlist in source.playlists.values() {
            if playlist
                .tracks
                .iter()
                .chain(
                    playlist
                        .imports
                        .iter()
                        .filter_map(|i| i.track_id.as_ref().map(|id| &id.0)),
                )
                .any(|id| !source.tracks.contains_key(id))
            {
                return Err(Error::Invalid("Playlist references a missing track".into()));
            }
        }
        for (&id, entry) in &source.tracks {
            check(&active)?;
            validate_extension(&entry.extension)?;
            let mut record: PortableTrack =
                serde_json::from_slice(&pack.bytes(&entry.record, 64 * 1024 * 1024, &active)?)?;
            if record.track.id.0 != id
                || record.track.content_hash != format!("blake3-{}", entry.audio.hash)
            {
                return Err(Error::Invalid("Track identity mismatch".into()));
            }
            validate_record(&record)?;
            progress(format!(
                "Restoring {} · {}",
                report.tracks + 1,
                record.track.title
            ));
            let dir = staging.path().join(format!("{source_id}-{id}"));
            fs::create_dir(&dir)?;
            let audio = dir.join(format!("audio.{}", entry.extension));
            extract_file(&mut pack, &entry.audio, &audio, &active)?;
            if let Some(art) = &entry.artwork {
                extract_file(&mut pack, art, &dir.join("artwork"), &active)?;
            }
            let duration = record
                .analysis
                .as_ref()
                .map_or(record.track.duration_ms as f32 / 1000., |(_, a)| {
                    a.duration_sec
                });
            if entry
                .stems
                .as_ref()
                .is_some_and(|s| s.len() != 4 && s.len() != 5)
            {
                return Err(Error::Invalid("Invalid stem cache file count".into()));
            }
            if let Some(cached) = entry.stems.as_ref().filter(|s| s.len() == 5) {
                if cached[0].size > 64 * 1024 * 1024 {
                    return Err(Error::Invalid("Stem metadata exceeds 64 MiB".into()));
                }
                let cache = dir.join("stems");
                fs::create_dir(&cache)?;
                for (blob, name) in cached.iter().zip(mixless_stems::PORTABLE_FILES) {
                    extract_file(&mut pack, blob, &cache.join(name), &active)?;
                }
                Processor::validate_portable_cache(&cache, duration, &active)?;
                let evidence = serde_json::from_slice(&fs::read(cache.join("analysis.json"))?)?;
                if let Some((_, analysis)) = &mut record.analysis {
                    analysis.stems = Some(evidence);
                }
                report.stems += 1;
            } else {
                // Legacy three-lane PCM is regenerated. Keep its evidence so
                // the background upgrade cannot weight adjusted bars twice.
                if entry.stems.is_none() {
                    if let Some((_, analysis)) = &mut record.analysis {
                        analysis.stems = None;
                    }
                }
                report.without_stems += 1;
            }
            report.without_analysis += usize::from(record.analysis.is_none());
            report.tracks += 1;
            fs::write(dir.join("record.json"), serde_json::to_vec(&record)?)?;
            staged.push((source_id.clone(), id, dir, duration));
        }
        report.playlists += source.playlists.len();
    }
    // Publish immutable media. If the database fails, only newly-created media
    // is removed; content-addressed stem caches may safely remain for reuse.
    let mut published = Published::default();
    for (source, id, dir, duration) in &staged {
        check(&active)?;
        let entry = &sources[source].tracks[id];
        let mut record: PortableTrack =
            serde_json::from_slice(&fs::read(dir.join("record.json"))?)?;
        let name = format!("{source}-{id}-{}.{}", entry.audio.hash, entry.extension);
        let target = root.join(name);
        publish(
            &dir.join(format!("audio.{}", entry.extension)),
            &target,
            &entry.audio.hash,
            &mut published,
        )?;
        record.track.path = target.canonicalize()?.to_string_lossy().into_owned();
        record.track.artwork_path = if let Some(art) = &entry.artwork {
            let target = root.join(format!("art-{}", art.hash));
            publish(&dir.join("artwork"), &target, &art.hash, &mut published)?;
            Some(target.canonicalize()?.to_string_lossy().into_owned())
        } else {
            None
        };
        if entry.stems.as_ref().is_some_and(|s| s.len() == 5) {
            stems.install_portable_cache(
                &record.track.content_hash,
                *duration,
                &dir.join("stems"),
                &active,
            )?;
        }
        fs::write(dir.join("record.json"), serde_json::to_vec(&record)?)?;
    }
    check(&active)?;
    #[cfg(unix)]
    File::open(&root)?.sync_all()?;
    progress("Restoring playlists".into());
    // One database transaction covers all source libraries in this package.
    let catalogs: Vec<_> = sources
        .iter()
        .map(|(source, s)| {
            (
                source.as_str(),
                s.order.as_slice(),
                s.playlists.values().cloned().collect::<Vec<_>>(),
            )
        })
        .collect();
    report.imported = library.restore_portable_sources(&catalogs, |source, id| {
        let bytes = fs::read(staging.path().join(format!("{source}-{id}/record.json")))?;
        Ok(serde_json::from_slice(&bytes)?)
    })?;
    published.committed = true;
    Ok(report)
}

fn validate_record(record: &PortableTrack) -> Result<()> {
    if let Some((_, a)) = &record.analysis {
        if a.track_id != record.track.id
            || !a.duration_sec.is_finite()
            || a.duration_sec <= 0.
            || !(8000..=384000).contains(&a.sample_rate)
        {
            return Err(Error::Invalid("Invalid track analysis".into()));
        }
    }
    if let Some(w) = &record.waveform {
        if !w.duration.is_finite() || w.duration <= 0. {
            return Err(Error::Invalid("Invalid waveform".into()));
        }
    }
    let mut cues = HashSet::new();
    if record
        .cues
        .iter()
        .any(|c| c.index >= 8 || c.frame > i64::MAX as u64 || !cues.insert(c.index))
    {
        return Err(Error::Invalid("Invalid cues".into()));
    }
    Ok(())
}
fn extract_file(
    pack: &mut Container,
    blob: &Blob,
    path: &Path,
    active: &impl Fn() -> bool,
) -> Result<()> {
    let mut file = File::create(path)?;
    pack.extract(blob, &mut file, active)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}
#[derive(Default)]
struct Published {
    paths: Vec<PathBuf>,
    committed: bool,
}
impl Drop for Published {
    fn drop(&mut self) {
        if !self.committed {
            for path in &self.paths {
                let _ = fs::remove_file(path);
            }
        }
    }
}
fn publish(source: &Path, target: &Path, hash: &str, guard: &mut Published) -> Result<()> {
    if target.exists() {
        if fs::symlink_metadata(target)?.file_type().is_symlink()
            || mixless_library::content_hash(target)? != format!("blake3-{hash}")
        {
            return Err(Error::Invalid("Existing imported media is damaged".into()));
        }
    } else {
        // hard_link refuses to overwrite and stays on the same filesystem.
        fs::hard_link(source, target)?;
        guard.paths.push(target.to_path_buf());
    }
    Ok(())
}
