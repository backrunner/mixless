use crate::{Error, Result, Stems};
use mixless_protocol::StemAnalysis;
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn key(content: &str) -> String {
    blake3::hash(
        format!(
            "{}:{}:{}:{content}",
            crate::evidence::VERSION,
            crate::models::SEPARATOR_HASH,
            crate::models::NOTES_HASH
        )
        .as_bytes(),
    )
    .to_hex()
    .to_string()
}
pub fn read(dir: &Path, duration: f32) -> Result<Option<StemAnalysis>> {
    let data = match fs::read(dir.join("analysis.json")) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let analysis: StemAnalysis = match serde_json::from_slice(&data) {
        Ok(a) => a,
        Err(_) => return Ok(None),
    };
    if analysis.version != crate::evidence::VERSION
        || analysis.separator_sha256 != crate::models::SEPARATOR_HASH
        || analysis.notes_sha256 != crate::models::NOTES_HASH
        || !analysis.valid(duration)
    {
        return Ok(None);
    }
    // A manifest alone cannot make an incomplete audio cache usable.
    for name in ["vocals", "drums", "instruments"] {
        let path = dir.join(format!("{name}.wav"));
        let Ok(wav) = hound::WavReader::open(&path) else {
            return Ok(None);
        };
        if wav.spec().channels != 2
            || wav.spec().sample_rate != 44100
            || (wav.duration() as f32 / 44100. - duration).abs() > 0.05
            || fs::metadata(path)?.len() < wav.duration() as u64 * 8
        {
            return Ok(None);
        }
    }
    let _ = fs::File::open(dir.join("analysis.json"))
        .and_then(|f| f.set_times(fs::FileTimes::new().set_modified(std::time::SystemTime::now())));
    Ok(Some(analysis))
}

pub fn save(dir: &Path, stems: &Stems, analysis: &StemAnalysis) -> Result<()> {
    let root = dir
        .parent()
        .ok_or_else(|| Error::Model("Cache path has no parent".into()))?;
    fs::create_dir_all(root)?;
    let temp = dir.with_extension(format!("{}.partial", std::process::id()));
    if temp.exists() {
        fs::remove_dir_all(&temp)?;
    }
    fs::create_dir(&temp)?;
    let result = (|| {
        for (i, name) in ["vocals", "drums", "instruments"].iter().enumerate() {
            let path = temp.join(format!("{name}.wav"));
            let mut wav = hound::WavWriter::create(
                &path,
                hound::WavSpec {
                    channels: 2,
                    sample_rate: 44100,
                    bits_per_sample: 32,
                    sample_format: hound::SampleFormat::Float,
                },
            )?;
            for &sample in &stems.audio[i] {
                wav.write_sample(sample)?;
            }
            wav.finalize()?;
            fs::File::open(path)?.sync_all()?;
        }
        fs::write(temp.join("analysis.json"), serde_json::to_vec(analysis)?)?;
        fs::File::open(temp.join("analysis.json"))?.sync_all()?;
        if dir.exists() {
            fs::remove_dir_all(dir)?;
        }
        fs::rename(&temp, dir)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    result
}

pub fn prune(root: &Path, keep: &Path) {
    const LIMIT: u64 = 6 * 1024 * 1024 * 1024;
    let mut entries: Vec<(std::time::SystemTime, u64, PathBuf)> = fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let p = entry.path();
            let name = p.file_name()?.to_str()?;
            if name.len() != 64 || !name.bytes().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let bytes = fs::read_dir(&p)
                .ok()?
                .flatten()
                .filter_map(|e| e.metadata().ok().map(|m| m.len()))
                .sum();
            Some((
                fs::metadata(p.join("analysis.json"))
                    .ok()?
                    .modified()
                    .ok()?,
                bytes,
                p,
            ))
        })
        .collect();
    entries.sort_by_key(|e| e.0);
    let mut total: u64 = entries.iter().map(|e| e.1).sum();
    for (_, size, path) in entries {
        if total <= LIMIT {
            break;
        }
        if path != keep && fs::remove_dir_all(path).is_ok() {
            total = total.saturating_sub(size);
        }
    }
}

pub fn reserve(root: &Path, bytes: u64) -> Result<()> {
    const HEADROOM: u64 = 512 * 1024 * 1024;
    if fs2::available_space(root)? > bytes + HEADROOM {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(root)?
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?;
            if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            Some((
                fs::metadata(path.join("analysis.json"))
                    .ok()?
                    .modified()
                    .ok()?,
                path,
            ))
        })
        .collect();
    entries.sort_by_key(|e| e.0);
    for (_, path) in entries {
        fs::remove_dir_all(path)?;
        if fs2::available_space(root)? > bytes + HEADROOM {
            return Ok(());
        }
    }
    Err(Error::Model(
        "Not enough free space for the stem cache; basic analysis remains available".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn partial_corrupt_and_different_content_caches_do_not_count_as_ready() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(key("a"));
        let stems = Stems {
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
}
