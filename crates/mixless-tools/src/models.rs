//! Release model preparation uses the same pinned identities as inference.
use mixless_protocol::{ModelArtifact, STEM_MODELS};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn valid(dir: &Path, model: &ModelArtifact) -> Result<bool> {
    let mut file = match fs::File::open(dir.join(model.name)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    let mut digest = Sha256::new();
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(format!("{:x}", digest.finalize()) == model.sha256)
}

pub fn verify(dir: &Path) -> Result<()> {
    for model in &STEM_MODELS {
        if !valid(dir, model)? {
            return Err(format!(
                "Missing or damaged model: {}",
                dir.join(model.name).display()
            )
            .into());
        }
    }
    Ok(())
}

pub fn download(dir: &Path) -> Result<()> {
    fs::create_dir_all(dir)?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()?;
    for model in &STEM_MODELS {
        if valid(dir, model)? {
            continue;
        }
        eprintln!("Downloading {}", model.name);
        let temp_dir = dir.join(format!(".download-{}", std::process::id()));
        fs::create_dir_all(&temp_dir)?;
        let result = (|| -> Result<()> {
            let mut response = client
                .get(model.url)
                .send()?
                .error_for_status()?
                .take(model.max_size + 1);
            let mut file = fs::File::create(temp_dir.join(model.name))?;
            let written = std::io::copy(&mut response, &mut file)?;
            file.flush()?;
            file.sync_all()?;
            if written > model.max_size || !valid(&temp_dir, model)? {
                return Err(format!("Model size or checksum mismatch: {}", model.name).into());
            }
            fs::rename(temp_dir.join(model.name), dir.join(model.name))?;
            Ok(())
        })();
        let _ = fs::remove_dir_all(temp_dir);
        result?;
    }
    verify(dir)
}

pub fn bundle(source: Option<&Path>, dest: &Path) -> Result<()> {
    if let Some(source) = source {
        verify(source)?;
        fs::create_dir_all(dest)?;
        for model in &STEM_MODELS {
            super::atomic_copy(&source.join(model.name), &dest.join(model.name))?;
        }
        verify(dest)?;
    } else if dest.exists() {
        // Reusing an output directory must not accidentally turn standard into bundled.
        fs::remove_dir_all(dest)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packaging_rejects_partial_or_corrupt_models_and_standard_removes_old_weights() {
        let source = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let dest = output.path().join("models");
        assert!(bundle(Some(source.path()), &dest).is_err());
        assert!(!dest.exists());
        for model in &STEM_MODELS {
            fs::write(source.path().join(model.name), b"corrupt").unwrap();
        }
        assert!(bundle(Some(source.path()), &dest).is_err());
        assert!(!dest.exists());
        fs::create_dir_all(&dest).unwrap();
        fs::write(dest.join(STEM_MODELS[0].name), b"old").unwrap();
        bundle(None, &dest).unwrap();
        assert!(!dest.exists());
    }
}
