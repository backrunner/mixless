use super::*;

/// Selecting playlists merges them into an existing package. Whole-library
/// export replaces this source's snapshot, preserving any other source libraries.
pub fn export(
    library: &Library,
    stems: &Processor,
    path: &Path,
    selection: Selection,
    mut progress: impl FnMut(String),
    active: impl Fn() -> bool,
) -> Result<Report> {
    check(&active)?;
    let catalog = library.portable_catalog(match &selection {
        Selection::Library => None,
        Selection::Playlists(ids) => Some(ids),
    })?;
    let mut pack = Container::open(path, true)?;
    let mut source = match selection {
        Selection::Library => Source::default(),
        _ => pack
            .manifest
            .sources
            .remove(&catalog.source)
            .unwrap_or_default(),
    };
    let mut report = Report {
        tracks: catalog.tracks.len(),
        playlists: catalog.playlists.len(),
        ..Default::default()
    };
    let mut order: Vec<_> = catalog.tracks.iter().map(|t| t.id.0).collect();
    let selected: HashSet<_> = order.iter().copied().collect();
    order.extend(
        source
            .order
            .iter()
            .filter(|id| !selected.contains(id))
            .copied(),
    );
    for (index, track) in catalog.tracks.iter().enumerate() {
        check(&active)?;
        progress(format!(
            "Packing {}/{} · {}",
            index + 1,
            report.tracks,
            track.title
        ));
        let mut record = library.portable_track(track)?;
        let audio = pack.add(File::open(&track.path)?, &active)?;
        let expected = format!("blake3-{}", audio.hash);
        if !track.content_hash.is_empty() && track.content_hash != expected {
            return Err(Error::Invalid(format!(
                "{} changed or has not been analyzed; refresh it before export",
                track.title
            )));
        }
        record.track.content_hash = expected;
        let extension = Path::new(&track.path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("audio")
            .to_ascii_lowercase();
        validate_extension(&extension)?;
        let artwork = record
            .track
            .artwork_path
            .as_ref()
            .map(|path| {
                File::open(path)
                    .map_err(Error::from)
                    .and_then(|file| pack.add(file, &active))
            })
            .transpose()?;
        let duration = record
            .analysis
            .as_ref()
            .map_or(track.duration_ms as f32 / 1000., |(_, a)| a.duration_sec);
        let cached = stems
            .with_portable_cache(
                &record.track.content_hash,
                duration,
                &active,
                |dir| -> Result<Vec<Blob>> {
                    Processor::validate_portable_cache(dir, duration, &active)?;
                    let mut files = Vec::new();
                    for name in mixless_stems::PORTABLE_FILES {
                        files.push(pack.add(File::open(dir.join(name))?, &active)?);
                    }
                    Ok(files)
                },
            )?
            .transpose()?;
        report.stems += usize::from(cached.is_some());
        report.without_stems += usize::from(cached.is_none());
        report.without_analysis += usize::from(record.analysis.is_none());
        // Portable metadata never carries absolute source-machine paths.
        record.track.path.clear();
        record.track.artwork_path = None;
        if let Some((_, a)) = &mut record.analysis {
            a.waveform_path = None;
            if cached.is_none() {
                a.stems = None;
            }
        }
        let bytes = serde_json::to_vec(&record)?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err(Error::Invalid("Track metadata exceeds 64 MiB".into()));
        }
        let record = pack.add(bytes.as_slice(), &active)?;
        source.tracks.insert(
            track.id.0,
            PackedTrack {
                record,
                audio,
                extension,
                artwork,
                stems: cached,
            },
        );
    }
    for playlist in catalog.playlists {
        source.playlists.insert(playlist.id, playlist);
    }
    source.order = order;
    pack.manifest.sources.insert(catalog.source, source);
    check(&active)?;
    progress("Committing package".into());
    report.generation = pack.commit()?;
    report.added_bytes = pack.added;
    Ok(report)
}
