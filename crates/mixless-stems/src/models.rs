//! Versioned, checksum-verified artifacts. No audio is sent over the network.
use crate::{Error, Progress, Result};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

pub const SEPARATOR_HASH: &str = "d05c269d0178d2a72ad484b10b11dd370193fc923201c3b27a99f848745db70a";
pub const NOTES_HASH: &str = "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";
const SEPARATOR_URL: &str = "https://huggingface.co/StemSplitio/htdemucs-onnx/resolve/d54ed9eb60e258ea82131c6ee14578628816456a/htdemucs_fp16weights.onnx";
const NOTES_URL: &str = "https://raw.githubusercontent.com/spotify/basic-pitch/v0.4.0/basic_pitch/saved_models/icassp_2022/nmp.onnx";

pub struct Paths {
    pub separator: PathBuf,
    pub notes: PathBuf,
}

pub fn ensure(
    dir: &Path,
    download: bool,
    progress: &mut impl FnMut(Progress),
    active: &impl Fn() -> bool,
) -> Result<Paths> {
    std::fs::create_dir_all(dir)?;
    let _lock = crate::lock::acquire(&dir.join(".models.lock"), active)?;
    Ok(Paths {
        separator: artifact(
            dir,
            "htdemucs-fp16.onnx",
            SEPARATOR_URL,
            SEPARATOR_HASH,
            165_612_636,
            download,
            progress,
            active,
        )?,
        notes: artifact(
            dir,
            "basic-pitch.onnx",
            NOTES_URL,
            NOTES_HASH,
            20_000_000,
            download,
            progress,
            active,
        )?,
    })
}

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

#[allow(clippy::too_many_arguments)]
fn artifact(
    dir: &Path,
    name: &str,
    url: &str,
    hash: &str,
    max_size: u64,
    download: bool,
    progress: &mut impl FnMut(Progress),
    active: &impl Fn() -> bool,
) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn damaged_weights_never_reach_runtime_or_require_network_in_offline_mode() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("htdemucs-fp16.onnx"), b"broken").unwrap();
        assert!(ensure(dir.path(), false, &mut |_| {}, &|| true).is_err());
        assert!(matches!(
            ensure(dir.path(), true, &mut |_| {}, &|| false),
            Err(Error::Cancelled)
        ));
    }
}
