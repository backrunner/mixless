//! Blocking HTTP and digest work, used only on the updater worker.
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};

pub(super) fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
    client(60)?
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json::<T>())
        .map_err(|e| format!("GET {url}: {e}"))
}

fn client(timeout_secs: u64) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .user_agent(format!("mixless/{}", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(timeout_secs))
        .build()
        .map_err(|e| format!("Cannot initialize update client: {e}"))
}

pub(super) fn download(url: &str, dest: &Path, progress: impl Fn(u8)) -> Result<(), String> {
    progress(0);
    let mut last_percent = 0;
    let mut response = client(900)?
        .get(url)
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = response.content_length().unwrap_or(0);
    let mut file =
        fs::File::create(dest).map_err(|e| format!("Cannot write {}: {e}", dest.display()))?;
    let mut written = 0u64;
    let mut buf = [0u8; 262144];
    loop {
        let n = response
            .read(&mut buf)
            .map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("Cannot write {}: {e}", dest.display()))?;
        written += n as u64;
        if total > 0 {
            let percent = (written.saturating_mul(100) / total).min(99) as u8;
            if percent != last_percent {
                progress(percent);
                last_percent = percent;
            }
        }
    }
    if total > 0 && written != total {
        return Err("Download truncated".into());
    }
    file.sync_all()
        .map_err(|e| format!("Cannot flush download: {e}"))?;
    progress(100);
    Ok(())
}

pub(super) fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let mut file =
        fs::File::open(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buf = [0u8; 262144];
    loop {
        let n = file
            .read(&mut buf)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    let actual = format!("{:x}", digest.finalize());
    if actual != expected.to_ascii_lowercase() {
        return Err("Update checksum mismatch".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "network_tests.rs"]
mod tests;
