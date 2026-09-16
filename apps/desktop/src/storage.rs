//! Disk usage reporting and user-initiated cache cleanup. Everything listed
//! here is regenerable — source audio, saved cues and playlists are never
//! removed, and nothing is evicted without an explicit user action.
use std::path::Path;
use std::sync::atomic::Ordering;

use mixless_protocol::{PlaylistId, TrackId};

use crate::state::AppCore;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ClearKind {
    Stems,
    Models,
    Artwork,
    Waveforms,
    Analysis,
    Playlist(i64),
}

pub struct PlaylistStorage {
    pub id: i64,
    pub name: String,
    pub tracks: usize,
    pub bytes: u64,
}

#[derive(Default)]
pub struct StorageReport {
    pub stems: u64,
    pub models: u64,
    pub artwork: u64,
    pub waveforms: u64,
    pub analysis: u64,
    pub database: u64,
    pub acquired: u64,
    pub playlists: Vec<PlaylistStorage>,
}

/// Recursive file total without following links.
fn dir_bytes(path: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                pending.push(entry.path());
            } else if kind.is_file() {
                total += entry.metadata().map(|m| m.len()).unwrap_or(0);
            }
        }
    }
    total
}

/// Scan the data directory off the UI thread; large stem caches take a moment.
pub fn report(core: &AppCore) -> Result<StorageReport, String> {
    let usage = core.library.cache_usage(None).map_err(|e| e.to_string())?;
    let summaries = core.library.list_playlists().map_err(|e| e.to_string())?;
    let mut playlists = Vec::with_capacity(summaries.len());
    for summary in &summaries {
        let id = PlaylistId(summary.id);
        let keys = core
            .library
            .cache_keys(Some(id))
            .map_err(|e| e.to_string())?;
        let hashes: Vec<String> = keys.into_iter().map(|(_, hash)| hash).collect();
        let usage = core
            .library
            .cache_usage(Some(id))
            .map_err(|e| e.to_string())?;
        playlists.push(PlaylistStorage {
            id: summary.id,
            name: crate::views::library::folder_name(summary, &summaries),
            tracks: usage.tracks,
            bytes: usage.analysis_bytes
                + usage.waveform_bytes
                + core
                    .stems
                    .as_ref()
                    .map_or(0, |s| s.cache_usage(Some(&hashes)))
                + core.library.artwork_bytes(Some(&hashes)),
        });
    }
    Ok(StorageReport {
        stems: core.stems.as_ref().map_or(0, |s| s.cache_usage(None)),
        models: core.stems.as_ref().map_or(0, |s| dir_bytes(s.model_dir())),
        artwork: core.library.artwork_bytes(None),
        waveforms: usage.waveform_bytes,
        analysis: usage.analysis_bytes,
        database: core.library.database_bytes(),
        acquired: dir_bytes(&core.download_dir()),
        playlists,
    })
}

/// Run one user-requested cleanup on a worker thread and return freed bytes.
/// Analysis state is forgotten so cleared tracks recompute on their next visit.
pub fn clear(core: &AppCore, kind: ClearKind) -> Result<u64, String> {
    let freed = match kind {
        ClearKind::Stems => stems(core)?.clear_cache(None).map_err(|e| e.to_string())?,
        ClearKind::Models => stems(core)?.clear_models().map_err(|e| e.to_string())?,
        ClearKind::Artwork => core
            .library
            .clear_artwork(None)
            .map_err(|e| e.to_string())?,
        ClearKind::Waveforms => core
            .library
            .clear_waveforms(None)
            .map_err(|e| e.to_string())?,
        ClearKind::Analysis => {
            let freed = core
                .library
                .clear_analysis(None)
                .map_err(|e| e.to_string())?;
            let keys = core.library.cache_keys(None).map_err(|e| e.to_string())?;
            forget(core, keys);
            freed
        }
        ClearKind::Playlist(id) => {
            let playlist = PlaylistId(id);
            let keys = core
                .library
                .cache_keys(Some(playlist))
                .map_err(|e| e.to_string())?;
            let hashes: Vec<String> = keys.iter().map(|(_, hash)| hash.clone()).collect();
            let mut freed = core
                .library
                .clear_waveforms(Some(playlist))
                .map_err(|e| e.to_string())?
                + core
                    .library
                    .clear_analysis(Some(playlist))
                    .map_err(|e| e.to_string())?
                + core
                    .library
                    .clear_artwork(Some(&hashes))
                    .map_err(|e| e.to_string())?;
            if let Some(stems) = &core.stems {
                freed += stems
                    .clear_cache(Some(&hashes))
                    .map_err(|e| e.to_string())?;
            }
            forget(core, keys);
            freed
        }
    };
    Ok(freed)
}

fn stems(core: &AppCore) -> Result<&mixless_stems::Processor, String> {
    core.stems
        .as_ref()
        .ok_or_else(|| "Stem analysis is not available".into())
}

/// Drop in-memory analysis for cleared tracks so a later visit recomputes it.
/// Loaded decks keep playing; their prepared state stays until eject.
fn forget(core: &AppCore, keys: Vec<(TrackId, String)>) {
    if keys.is_empty() {
        return;
    }
    for (id, _) in keys {
        core.analysis.forget(id);
        if let Ok(track) = core.library.get_track(id) {
            core.analysis.updated(track);
        }
    }
    core.analysis
        .content_revision
        .fetch_add(1, Ordering::AcqRel);
}

/// Short human size for cache rows: 0 B, 900 KB, 1.4 GB.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024. && unit < UNITS.len() - 1 {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_bytes_stays_compact() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(6 * 1024 * 1024 * 1024), "6.0 GB");
    }
}
