//! Inspect metadata directly: normal stem cache reads update LRU timestamps.
use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ModelFile {
    pub name: String,
    pub sha256: String,
    pub bytes: u64,
    pub modified: u128,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Assets {
    pub models: Vec<ModelFile>,
    pub caches: Vec<CacheFile>,
    pub incompatible_models: Vec<PathBuf>,
    pub incompatible_stems: Vec<PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct CacheFile {
    pub name: String,
    bytes: u64,
    modified: u128,
    header: Header,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Header {
    version: u32,
    #[serde(default)]
    separator_sha256: String,
    #[serde(default)]
    notes_sha256: String,
    storage: Option<Storage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Storage {
    version: u32,
    #[serde(default)]
    format: String,
}

pub(super) fn inspect(
    data: &Path,
    models: &Path,
    previous: Option<&super::versions::Stamp>,
) -> io::Result<Assets> {
    let mut result = Assets::default();
    for artifact in mixless_protocol::STEM_MODELS {
        let path = models.join(artifact.name);
        let meta = match fs::metadata(&path) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        let modified = meta
            .modified()?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos();
        let cached = previous.and_then(|p| {
            p.models.iter().find(|m| {
                m.name == artifact.name && m.bytes == meta.len() && m.modified == modified
            })
        });
        let sha256 = if let Some(cached) = cached {
            cached.sha256.clone()
        } else {
            let mut file = fs::File::open(&path)?;
            let mut hash = Sha256::new();
            let mut buffer = [0; 65536];
            loop {
                let count = file.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
            }
            hash.finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        if sha256 != artifact.sha256 {
            result.incompatible_models.push(path);
        }
        result.models.push(ModelFile {
            name: artifact.name.into(),
            sha256,
            bytes: meta.len(),
            modified,
        });
    }
    // Old model names are scoped to this directory. Never trust paths from a
    // foreign/future ledger as arbitrary filesystem paths.
    if let Some(previous) = previous {
        for model in &previous.models {
            if safe_name(&model.name)
                && !mixless_protocol::STEM_MODELS
                    .iter()
                    .any(|m| m.name == model.name)
                && models.join(&model.name).is_file()
            {
                result.incompatible_models.push(models.join(&model.name));
            }
        }
    }
    let cache = data.join("stem-cache");
    if cache.exists() {
        for entry in fs::read_dir(&cache)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.len() != 64
                || !name.bytes().all(|b| b.is_ascii_hexdigit())
                || !entry.file_type()?.is_dir()
            {
                continue;
            }
            let path = entry.path().join("analysis.json");
            let meta = match fs::metadata(&path) {
                Ok(meta) => meta,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e),
            };
            let modified = meta
                .modified()?
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(io::Error::other)?
                .as_nanos();
            let cached = previous.and_then(|p| {
                p.caches
                    .iter()
                    .find(|c| c.name == name && c.bytes == meta.len() && c.modified == modified)
            });
            // Stream past frames/notes without allocating their potentially
            // large payloads. Unchanged files reuse inspected version metadata.
            // Malformed data is an ordinary corrupt cache miss.
            let header = if let Some(cached) = cached {
                Some(cached.header.clone())
            } else {
                serde_json::from_reader::<_, Header>(io::BufReader::new(fs::File::open(&path)?))
                    .ok()
            };
            if let Some(header) = header {
                let future = header.version > mixless_stems::VERSION
                    || (header.version == mixless_stems::VERSION
                        && (header.separator_sha256 != mixless_protocol::STEM_SEPARATOR_HASH
                            || header.notes_sha256 != mixless_protocol::STEM_NOTES.sha256))
                    || header.storage.as_ref().is_some_and(|s| {
                        s.version > mixless_stems::STORAGE_VERSION
                            || serde_json::from_value::<mixless_stems::StorageFormat>(
                                serde_json::Value::String(s.format.clone()),
                            )
                            .is_err()
                    });
                if future {
                    result.incompatible_stems.push(entry.path());
                }
                result.caches.push(CacheFile {
                    name: name.into_owned(),
                    bytes: meta.len(),
                    modified,
                    header,
                });
            } else {
                // An unknown manifest shape may belong to a future version.
                // Do not silently overwrite it as an ordinary cache miss.
                result.incompatible_stems.push(entry.path());
            }
        }
    }
    result.incompatible_stems.sort();
    result.caches.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && Path::new(name).file_name().is_some_and(|n| n == name)
        && Path::new(name).extension().is_some_and(|ext| ext == "onnx")
        && !name.contains(['/', '\\'])
        && name != "."
        && name != ".."
}

/// A reversible move journal. If conversion or a later move fails, restore
/// already-moved assets before allowing another startup attempt.
#[derive(Default)]
pub(super) struct Moves(Vec<(PathBuf, PathBuf)>);

impl Moves {
    pub fn move_to(&mut self, source: &Path, destination: &Path) -> io::Result<()> {
        if !source.exists() {
            return Ok(());
        }
        fs::create_dir_all(destination.parent().expect("backup parent"))?;
        match fs::rename(source, destination) {
            Ok(()) => {}
            // External model dirs can be on another volume. Copy before
            // unlinking, so an incomplete backup never deletes the original.
            Err(e) if e.raw_os_error() == Some(libc_exdev()) && source.is_file() => {
                fs::copy(source, destination)?;
                fs::File::open(destination)?.sync_all()?;
                fs::remove_file(source)?;
            }
            Err(e) => return Err(e),
        }
        self.0.push((source.into(), destination.into()));
        Ok(())
    }

    pub fn commit(mut self) {
        self.0.clear();
    }
}

fn libc_exdev() -> i32 {
    18
}

impl Drop for Moves {
    fn drop(&mut self) {
        for (source, destination) in self.0.iter().rev() {
            if let Err(error) = fs::rename(destination, source).or_else(|e| {
                if e.raw_os_error() == Some(libc_exdev()) && destination.is_file() {
                    fs::copy(destination, source)?;
                    fs::remove_file(destination)
                } else {
                    Err(e)
                }
            }) {
                tracing::error!(%error, backup=%destination.display(), "could not restore downgrade backup");
            }
        }
    }
}
