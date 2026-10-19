use crate::storage::{Manifest, NAMES};
#[cfg(stems_ort)]
use crate::storage::{Storage, StorageFormat, RATE};
use crate::Result;
#[cfg(stems_ort)]
use crate::{Error, Stems};
use mixless_protocol::StemAnalysis;
use std::{fs, path::Path};

pub fn key(content: &str) -> String {
    key_version(content, crate::evidence::VERSION)
}
pub fn key_version(content: &str, version: u32) -> String {
    blake3::hash(
        format!(
            "{}:{}:{}:{content}",
            version,
            crate::models::SEPARATOR_HASH,
            crate::models::NOTES_HASH
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string()
}
pub fn read(dir: &Path, duration: f32) -> Result<Option<StemAnalysis>> {
    read_version(dir, duration, crate::evidence::VERSION)
}
pub fn read_legacy(dir: &Path, duration: f32) -> Result<Option<StemAnalysis>> {
    read_version(dir, duration, 1)
}
fn read_version(dir: &Path, duration: f32, version: u32) -> Result<Option<StemAnalysis>> {
    let data = match fs::read(dir.join("analysis.json")) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let manifest: Manifest = match serde_json::from_slice(&data) {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };
    let analysis = &manifest.analysis;
    if analysis.version != version
        || analysis.separator_sha256 != crate::models::SEPARATOR_HASH
        || analysis.notes_sha256 != crate::models::NOTES_HASH
        || !analysis.valid(duration)
    {
        return Ok(None);
    }
    if let Some(storage) = &manifest.storage {
        if version != crate::VERSION || !storage.valid(duration) {
            return Ok(None);
        }
        for (i, name) in manifest.files()[1..].iter().enumerate() {
            let path = dir.join(name);
            if fs::metadata(&path).map(|m| m.len()).ok() != Some(storage.bytes[i])
                || crate::codec::probe(&path, storage.format).ok() != Some(storage.frames)
            {
                return Ok(None);
            }
        }
    } else {
        // Legacy float WAV caches remain readable without being rewritten.
        for name in NAMES {
            if version == 1 && name == "bass" {
                continue;
            }
            let path = dir.join(format!("{name}.wav"));
            let Ok(wav) = hound::WavReader::open(&path) else {
                return Ok(None);
            };
            if wav.spec().channels != 2
                || wav.spec().sample_rate != 44100
                || wav.spec().bits_per_sample != 32
                || wav.spec().sample_format != hound::SampleFormat::Float
                || (wav.duration() as f32 / 44100. - duration).abs() > 0.05
                || fs::metadata(path)?.len() < wav.duration() as u64 * 8
            {
                return Ok(None);
            }
        }
    }
    let _ = fs::File::open(dir.join("analysis.json"))
        .and_then(|f| f.set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now())));
    Ok(Some(manifest.analysis))
}

pub(crate) fn manifest(dir: &Path) -> Result<Manifest> {
    let data = fs::read(dir.join("analysis.json"))?;
    Ok(serde_json::from_slice(&data)?)
}

pub(crate) fn audio(
    dir: &Path,
    manifest: &Manifest,
    index: usize,
    active: &impl Fn() -> bool,
) -> Result<Vec<f32>> {
    let path = dir.join(&manifest.files()[index + 1]);
    let (frames, gain) = if let Some(storage) = &manifest.storage {
        if !storage.valid(manifest.analysis.duration_sec)
            || fs::metadata(&path)?.len() != storage.bytes[index]
            || digest(&path, active)? != storage.blake3[index]
        {
            return Err(crate::Error::Model("Corrupt stem cache".into()));
        }
        (storage.frames, storage.gain[index])
    } else {
        (hound::WavReader::open(&path)?.duration() as u64, 1.)
    };
    crate::codec::read(&path, manifest.format(), frames, gain, active)
}

fn digest(path: &Path, active: &impl Fn() -> bool) -> Result<String> {
    use std::io::Read;
    let mut file = fs::File::open(path)?;
    let mut hash = blake3::Hasher::new();
    let mut block = [0; 65536];
    loop {
        crate::check(active)?;
        let count = file.read(&mut block)?;
        if count == 0 {
            break;
        }
        hash.update(&block[..count]);
    }
    Ok(hash.finalize().to_hex().to_string())
}

#[cfg(all(test, stems_ort))]
pub fn save(dir: &Path, stems: &Stems, analysis: &StemAnalysis) -> Result<()> {
    save_as(dir, stems, analysis, StorageFormat::Wav, &|| true)
}

#[cfg(stems_ort)]
pub(crate) fn save_as(
    dir: &Path,
    stems: &Stems,
    analysis: &StemAnalysis,
    format: StorageFormat,
    active: &impl Fn() -> bool,
) -> Result<()> {
    let frames = stems.audio[0].len() as u64 / 2;
    if !crate::is_current(analysis)
        || frames == 0
        || (frames as f64 / RATE as f64 - analysis.duration_sec as f64).abs() > 0.05
        || stems.audio.iter().any(|s| s.len() != frames as usize * 2)
        || stems.bass.len() != frames as usize * 2
    {
        return Err(Error::Model("Invalid stem cache input".into()));
    }
    let root = dir
        .parent()
        .ok_or_else(|| Error::Model("Cache path has no parent".into()))?;
    fs::create_dir_all(root)?;
    let temp = tempfile::Builder::new()
        .prefix(".stems-")
        .suffix(".partial")
        .tempdir_in(root)?;
    let mut storage = Storage {
        version: 1,
        format,
        frames,
        gain: [1.; 4],
        bytes: [0; 4],
        blake3: Default::default(),
    };
    for (i, name) in NAMES.iter().enumerate() {
        let path = temp.path().join(format!("{name}.{}", format.extension()));
        let samples = if i == 3 { &stems.bass } else { &stems.audio[i] };
        storage.gain[i] = crate::codec::write(&path, samples, format, active)?;
        storage.bytes[i] = fs::metadata(&path)?.len();
        storage.blake3[i] = digest(&path, active)?;
        // Refuse to publish an encoder output that cannot be decoded with the
        // exact production timing/PCM contract, including the final samples.
        crate::codec::read(&path, format, frames, storage.gain[i], active).map_err(
            |e| match e {
                Error::Cancelled => Error::Cancelled,
                _ => Error::Model(format!("{name}: {e}")),
            },
        )?;
        fs::File::open(path)?.sync_all()?;
    }
    let manifest = Manifest {
        analysis: analysis.clone(),
        storage: Some(storage),
    };
    fs::write(
        temp.path().join("analysis.json"),
        serde_json::to_vec(&manifest)?,
    )?;
    fs::File::open(temp.path().join("analysis.json"))?.sync_all()?;
    crate::check(active)?;
    if dir.exists() {
        fs::remove_dir_all(dir)?;
    }
    fs::rename(temp.path(), dir)?;
    #[cfg(unix)]
    fs::File::open(root)?.sync_all()?;
    Ok(())
}

/// Published entries are content-keyed 64-hex directories; `.partial`
/// siblings are interrupted saves, and other files are process locks.
fn is_published(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|c| c.is_ascii_hexdigit())
}

/// Recursive byte total; missing or unreadable entries count as zero.
pub fn dir_bytes(path: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![path.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
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

/// Remove published entries, stale partials and compiled CoreML models, returning freed
/// bytes. The caller holds the analysis lock, so no save can be in flight.
pub fn clear_all(root: &Path) -> Result<u64> {
    let mut freed = 0;
    for entry in fs::read_dir(root)?.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !is_published(name) && !name.ends_with(".partial") && name != ".coreml" {
            continue;
        }
        freed += dir_bytes(&path);
        if path.is_dir() {
            fs::remove_dir_all(path)?;
        } else {
            fs::remove_file(path)?;
        }
    }
    Ok(freed)
}

/// Fail when the disk cannot hold another entry plus headroom. Cache entries
/// are never evicted automatically — cleanup is explicit, from Preferences.
#[cfg(stems_ort)]
pub fn reserve(root: &Path, bytes: u64) -> Result<()> {
    const HEADROOM: u64 = 512 * 1024 * 1024;
    if fs2::available_space(root)? > bytes + HEADROOM {
        return Ok(());
    }
    Err(Error::Model(
        "Not enough free space for the stem cache; clear space or free some in Settings > Storage"
            .into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(stems_ort)]
    #[test]
    fn partial_corrupt_and_different_content_caches_do_not_count_as_ready() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(key("a"));
        let stems = Stems {
            bass: vec![0.; 4410],
            audio: std::array::from_fn(|_| vec![0.; 4410]),
            residual_rms: 0.,
        };
        let analysis = crate::evidence::extract(&stems, vec![]);
        save(&path, &stems, &analysis).unwrap();
        assert!(read(&path, 0.05).unwrap().is_some());
        assert!(read(&root.path().join(key("b")), 0.05).unwrap().is_none());
        assert!(read(&path, 0.1).unwrap().is_none());
        fs::OpenOptions::new()
            .write(true)
            .open(path.join("vocals.wav"))
            .unwrap()
            .set_len(100)
            .unwrap();
        assert!(read(&path, 0.05).unwrap().is_none());
    }

    #[test]
    fn usage_and_scoped_clearing_deduplicate_hashes_and_keep_locks() {
        let root = tempfile::tempdir().unwrap();
        let processor =
            crate::Processor::new(root.path().join("models"), root.path().join("cache"), false);
        for (hash, bytes) in [("aaa", 3usize), ("bbb", 5)] {
            let entry = processor.cache_path(hash);
            fs::create_dir_all(&entry).unwrap();
            fs::write(entry.join("vocals.wav"), vec![0; bytes]).unwrap();
        }
        let stale = processor.cache_path("ccc").with_extension("1.partial");
        fs::create_dir(&stale).unwrap();
        fs::write(stale.join("vocals.wav"), [0; 2]).unwrap();
        fs::write(root.path().join("cache/.analysis.lock"), b"").unwrap();
        fs::create_dir(root.path().join("cache/.coreml")).unwrap();
        fs::write(root.path().join("cache/.coreml/compiled"), [0; 11]).unwrap();

        assert_eq!(processor.cache_usage(None), 21);
        assert_eq!(
            processor.cache_usage(Some(&["aaa".into(), "aaa".into(), "none".into()])),
            3
        );
        assert_eq!(processor.clear_cache(Some(&["aaa".into()])).unwrap(), 3);
        assert!(!processor.cache_path("aaa").exists());
        assert!(root.path().join("cache/.coreml/compiled").exists());
        assert_eq!(processor.cache_usage(None), 18);
        assert_eq!(processor.clear_cache(None).unwrap(), 18);
        assert!(!root.path().join("cache/.coreml").exists());
        assert!(processor.cache_path("bbb").read_dir().is_err());
        // The process lock and a foreign file survive a full clear.
        assert!(root.path().join("cache/.analysis.lock").exists());
        assert_eq!(processor.cache_usage(None), 0);
    }

    #[test]
    fn pruning_keeps_referenced_current_cache_and_foreign_data() {
        let root = tempfile::tempdir().unwrap();
        let processor =
            crate::Processor::new(root.path().join("models"), root.path().join("cache"), false);
        for hash in ["keep", "orphan"] {
            let dir = processor.cache_path(hash);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("vocals.wav"), [0; 7]).unwrap();
        }
        let foreign = root.path().join("cache/user-folder");
        fs::create_dir(&foreign).unwrap();
        fs::write(foreign.join("keep"), [0; 3]).unwrap();
        assert_eq!(processor.prune_cache(&["keep".into()]).unwrap(), 7);
        assert!(processor.cache_path("keep").exists());
        assert!(!processor.cache_path("orphan").exists());
        assert!(foreign.join("keep").exists());
    }

    #[test]
    fn clearing_models_removes_artifacts_and_stale_partials() {
        let root = tempfile::tempdir().unwrap();
        let models = root.path().join("models");
        fs::create_dir(&models).unwrap();
        fs::write(models.join("htdemucs-fp16.onnx"), [0; 7]).unwrap();
        fs::write(models.join(".basic-pitch.onnx.1.part"), b"xx").unwrap();
        fs::write(models.join(".models.lock"), b"").unwrap();
        let processor = crate::Processor::new(models, root.path().join("cache"), false);
        assert_eq!(processor.clear_models().unwrap(), 9);
        let remaining: Vec<_> = processor
            .model_dir()
            .read_dir()
            .unwrap()
            .flatten()
            .collect();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].file_name().to_str(), Some(".models.lock"));
    }
}
