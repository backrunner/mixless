//! Versioned, checksum-verified artifacts. No audio is sent over the network.
#[cfg(stems_ort)]
use crate::{Error, Progress, Result};
#[cfg(stems_ort)]
use sha2::{Digest, Sha256};
#[cfg(stems_ort)]
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const SEPARATOR_HASH: &str = mixless_protocol::STEM_SEPARATOR.sha256;
pub const NOTES_HASH: &str = mixless_protocol::STEM_NOTES.sha256;
#[cfg(stems_ort)]
use mixless_protocol::{ModelArtifact, STEM_NOTES, STEM_SEPARATOR};

#[cfg(stems_ort)]
pub struct Paths {
    pub separator: PathBuf,
    pub notes: PathBuf,
}

#[cfg(stems_ort)]
pub fn ensure(
    dir: &Path,
    bundled: Option<&Path>,
    download: bool,
    progress: &mut impl FnMut(Progress),
    active: &impl Fn() -> bool,
) -> Result<Paths> {
    crate::check(active)?;
    // Signed app resources are read-only, including on the installer DMG.
    // Never put locks or temporary downloads inside the bundle.
    if let Some(bundled) = bundled {
        return Ok(Paths {
            separator: bundled_artifact(bundled, &STEM_SEPARATOR)?,
            notes: bundled_artifact(bundled, &STEM_NOTES)?,
        });
    }
    std::fs::create_dir_all(dir)?;
    let _lock = crate::lock::acquire(&dir.join(".models.lock"), active)?;
    Ok(Paths {
        separator: artifact(dir, &STEM_SEPARATOR, download, progress, active)?,
        notes: artifact(dir, &STEM_NOTES, download, progress, active)?,
    })
}

#[cfg(stems_ort)]
fn bundled_artifact(dir: &Path, model: &ModelArtifact) -> Result<PathBuf> {
    let path = dir.join(model.name);
    if !valid(&path, model.sha256)? {
        return Err(Error::Model(format!(
            "Bundled model missing or damaged: {}; reinstall the installer with models",
            model.name
        )));
    }
    Ok(path)
}

#[cfg(stems_ort)]
fn valid(path: &Path, expected: &str) -> Result<bool> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut digest = Sha256::new();
    let mut block = [0; 65536];
    loop {
        let n = file.read(&mut block)?;
        if n == 0 {
            break;
        }
        digest.update(&block[..n]);
    }
    Ok(format!("{:x}", digest.finalize()) == expected)
}

#[cfg(stems_ort)]
fn artifact(
    dir: &Path,
    model: &ModelArtifact,
    download: bool,
    progress: &mut impl FnMut(Progress),
    active: &impl Fn() -> bool,
) -> Result<PathBuf> {
    let ModelArtifact {
        name,
        url,
        sha256: hash,
        max_size,
    } = *model;
    crate::check(active)?;
    let path = dir.join(name);
    if valid(&path, hash)? {
        return Ok(path);
    }
    if !download {
        return Err(Error::Model(format!(
            "Missing or damaged model: {}",
            path.display()
        )));
    }
    progress(Progress::Downloading {
        model: name.into(),
        percent: 0,
    });
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()?;
    let mut response = client.get(url).send()?.error_for_status()?;
    let size = response.content_length().unwrap_or(max_size).min(max_size);
    let temp = dir.join(format!(".{name}.{}.part", std::process::id()));
    let result = (|| {
        let mut output = File::create(&temp)?;
        let mut written = 0u64;
        let mut buf = [0; 65536];
        loop {
            crate::check(active)?;
            let n = response.read(&mut buf)?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > max_size {
                return Err(Error::Model("Model exceeds declared size".into()));
            }
            output.write_all(&buf[..n])?;
            progress(Progress::Downloading {
                model: name.into(),
                percent: (100 * written / size.max(1)).min(100) as u8,
            });
        }
        output.sync_all()?;
        if !valid(&temp, hash)? {
            return Err(Error::Model(format!("Checksum mismatch: {name}")));
        }
        std::fs::rename(&temp, &path)?;
        Ok(path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

#[cfg(all(test, stems_ort))]
mod tests {
    use super::*;
    #[test]
    fn damaged_weights_never_reach_runtime_or_require_network_in_offline_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("htdemucs-fp16.onnx"), b"broken").unwrap();
        assert!(ensure(dir.path(), None, false, &mut |_| {}, &|| true).is_err());
        assert!(matches!(
            ensure(dir.path(), None, true, &mut |_| {}, &|| false),
            Err(Error::Cancelled)
        ));
    }

    #[test]
    fn bundled_models_are_read_only_and_never_fall_back_to_network() {
        let bundled = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let download_dir = cache.path().join("unused");
        let model = ModelArtifact {
            name: "test.onnx",
            url: "https://invalid.example",
            max_size: 3,
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        };
        std::fs::write(bundled.path().join(model.name), b"abc").unwrap();
        assert_eq!(
            bundled_artifact(bundled.path(), &model).unwrap(),
            bundled.path().join(model.name)
        );
        std::fs::write(bundled.path().join(model.name), b"damaged").unwrap();
        assert!(bundled_artifact(bundled.path(), &model).is_err());
        let mut progress = Vec::new();
        assert!(ensure(
            &download_dir,
            Some(bundled.path()),
            true,
            &mut |p| progress.push(p),
            &|| true
        )
        .is_err());
        assert!(progress.is_empty());
        assert!(!download_dir.exists());
        assert!(!bundled.path().join(".models.lock").exists());
    }
}
