//! Fetch verified native assets for packaging; never invoked by the installed app.
use mixless_protocol::inference_runtime::{ARCHIVES, VERSION};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn valid(path: &Path, expected: &str) -> Result<bool> {
    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    let mut hash = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()) == expected)
}

pub fn verify(dir: &Path) -> Result<()> {
    for archive in ARCHIVES {
        for &(name, hash) in archive.files {
            if !valid(&dir.join(name), hash)? {
                return Err(format!("Missing or damaged inference runtime: {}. Run prepare-inference-runtime first.", dir.join(name).display()).into());
            }
        }
    }
    Ok(())
}

pub fn prepare(root: &Path) -> Result<()> {
    let dir = root.join(VERSION);
    fs::create_dir_all(&dir)?;
    if verify(&dir).is_ok() {
        return Ok(());
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(600))
        .build()?;
    for archive in ARCHIVES {
        if archive
            .files
            .iter()
            .all(|(name, hash)| valid(&dir.join(name), hash).unwrap_or(false))
        {
            continue;
        }
        eprintln!("Preparing native inference runtime: {}", archive.prefix);
        let temp = tempfile::tempdir_in(root)?;
        let download = temp.path().join("archive.zip");
        let mut response = client
            .get(archive.url)
            .send()?
            .error_for_status()?
            .take(80 * 1024 * 1024 + 1);
        let mut file = fs::File::create(&download)?;
        let size = std::io::copy(&mut response, &mut file)?;
        file.flush()?;
        if size > 80 * 1024 * 1024 || !valid(&download, archive.sha256)? {
            return Err("Inference archive size/checksum mismatch".into());
        }
        let mut zip = zip::ZipArchive::new(fs::File::open(download)?)?;
        for &(name, hash) in archive.files {
            let mut entry = zip.by_name(&format!("{}{name}", archive.prefix))?;
            if entry.size() > 160 * 1024 * 1024 {
                return Err("Oversized inference library".into());
            }
            let extracted = temp.path().join(name);
            let mut output = fs::File::create(&extracted)?;
            std::io::copy(&mut entry, &mut output)?;
            output.sync_all()?;
            if !valid(&extracted, hash)? {
                return Err(format!("Inference library checksum mismatch: {name}").into());
            }
            super::atomic_copy(&extracted, &dir.join(name))?;
        }
    }
    verify(&dir)
}

pub fn bundle(source: &Path, frameworks: &Path, resources: &Path) -> Result<()> {
    verify(source)?;
    // Frameworks accepts dylibs directly, or proper .framework bundles. A loose
    // version subdirectory is treated as an invalid nested bundle by codesign.
    let dest = frameworks;
    fs::create_dir_all(dest)?;
    let old_layout = frameworks.join(VERSION);
    if old_layout.is_dir() {
        fs::remove_dir_all(old_layout)?;
    }
    let metal_dir = resources.join("inference-runtime");
    fs::create_dir_all(&metal_dir)?;
    // Metal libraries are resources, not Mach-O code objects. Keeping one in
    // Frameworks makes codesign reject the enclosing app as unsigned nested code.
    let old_metal = frameworks.join("mlx.metallib");
    if old_metal.is_file() {
        fs::remove_file(old_metal)?;
    }
    for archive in ARCHIVES {
        for &(name, hash) in archive.files {
            let target = if name.ends_with(".metallib") {
                metal_dir.join(name)
            } else {
                dest.join(name)
            };
            super::atomic_copy(&source.join(name), &target)?;
            if !valid(&target, hash)? {
                return Err(format!("Bundled inference library checksum mismatch: {name}").into());
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn incomplete_or_damaged_runtime_cannot_be_packaged() {
        let input = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        assert!(bundle(
            input.path(),
            &output.path().join("Frameworks"),
            &output.path().join("Resources")
        )
        .is_err());
        for artifact in ARCHIVES {
            for &(name, _) in artifact.files {
                fs::write(input.path().join(name), b"corrupt").unwrap();
            }
        }
        assert!(bundle(
            input.path(),
            &output.path().join("Frameworks"),
            &output.path().join("Resources")
        )
        .is_err());
        assert_eq!(fs::read_dir(output.path()).unwrap().count(), 0);
    }
}
