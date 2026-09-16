//! Self-update over the per-channel manifests published by the release
//! workflow. A check fetches the manifest for this build's channel and, when
//! it names a strictly newer version, downloads the DMG, verifies its hash and
//! signature, and installs it over the running bundle. It never downgrades and
//! never restarts the app mid-session — the banner asks the user to restart.

use std::env;
use std::sync::{Arc, Mutex};

use serde::Deserialize;

use crate::state::AppCore;

/// Shared updater slot on `AppCore`; `poll_update` renders it as a banner.
pub type Shared = Mutex<Option<Entry>>;

#[derive(Deserialize)]
struct Release {
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[derive(Deserialize)]
struct Manifest {
    channel: String,
    version: String,
    platforms: std::collections::HashMap<String, Platform>,
}

#[derive(Deserialize)]
struct Platform {
    url: String,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub status: Status,
    /// Menu-triggered checks surface every outcome; launch checks stay quiet
    /// unless real work is happening.
    pub manual: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Checking,
    Downloading { version: String, percent: u8 },
    Installing { version: String },
    /// Installed over the app; a restart finishes the update.
    Ready { version: String },
    UpToDate,
    Failed(String),
}

/// Manual "Check for Updates…" entry point. Single-flight.
pub fn start(core: Arc<AppCore>, manual: bool) {
    {
        let mut entry = core.update.lock().expect("update status");
        if matches!(
            entry.as_ref().map(|e| &e.status),
            Some(Status::Checking | Status::Downloading { .. } | Status::Installing { .. })
        ) {
            // A manual check upgrades an in-flight launch check so its
            // outcome is surfaced instead of swallowed.
            if let Some(e) = entry.as_mut() {
                e.manual |= manual;
            }
            return;
        }
        *entry = Some(Entry {
            status: Status::Checking,
            manual,
        });
    }
    std::thread::spawn(move || {
        let report = |status: Status| {
            *core.update.lock().expect("update status") = Some(Entry { status, manual });
        };
        let status = run(&report).unwrap_or_else(Status::Failed);
        report(status);
    });
}

/// Silent launch check. Dev builds and unpackaged runs are skipped.
pub fn launch(core: &Arc<AppCore>) {
    if env!("MIXLESS_CHANNEL") != "dev" {
        start(core.clone(), false);
    }
}

#[cfg(target_os = "macos")]
fn run(report: &dyn Fn(Status)) -> Result<Status, String> {
    macos::update(report)
}

#[cfg(not(target_os = "macos"))]
fn run(_report: &dyn Fn(Status)) -> Result<Status, String> {
    Err("Updates are only supported on macOS".into())
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{Manifest, Release, Status};
    use sha2::{Digest, Sha256};
    use std::{
        collections::HashMap,
        env, fs,
        io::{Read, Write},
        path::{Path, PathBuf},
        process::Command,
        time::Duration,
    };

    pub fn update(report: &dyn Fn(Status)) -> Result<Status, String> {
        let channel = env!("MIXLESS_CHANNEL");
        if channel == "dev" {
            return Err("Update checks require a signed release build".into());
        }
        let manifest = manifest(channel)?;
        if manifest.channel != channel {
            return Err(format!("Update feed channel mismatch: {}", manifest.channel));
        }
        let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
            .map_err(|e| format!("Unparsable build version: {e}"))?;
        let remote = semver::Version::parse(&manifest.version)
            .map_err(|e| format!("Unparsable manifest version {}: {e}", manifest.version))?;
        if remote <= current {
            return Ok(Status::UpToDate);
        }
        let platform = manifest
            .platforms
            .get("darwin-universal")
            .ok_or("Manifest has no universal macOS build")?;
        let work = env::temp_dir().join(format!("mixless-update-{}", std::process::id()));
        let _cleanup = WorkDir::create(&work)?;
        let dmg = work.join("update.dmg");
        download(&platform.url, &dmg, |percent| {
            report(Status::Downloading {
                version: manifest.version.clone(),
                percent,
            });
        })?;
        verify_sha256(&dmg, &platform.sha256)?;
        let mount = Mount::attach(&dmg, &work.join("mnt"))?;
        let app = find_app(mount.path())?;
        verify_bundle(&app, &manifest.version)?;
        report(Status::Installing {
            version: manifest.version.clone(),
        });
        let dest = update_target()?;
        crate::self_install::install_bundle(&app, &dest)
            .map_err(|e| format!("Install failed: {e}"))?;
        // Files copied off a DMG inherit quarantine; a notarized copy still
        // passes Gatekeeper if the removal is denied, so best effort.
        let _ = Command::new("/usr/bin/xattr")
            .args(["-dr", "com.apple.quarantine"])
            .arg(&dest)
            .status();
        Ok(Status::Ready {
            version: manifest.version,
        })
    }

    fn manifest(channel: &str) -> Result<Manifest, String> {
        let repo = env!("CARGO_PKG_REPOSITORY");
        let name = format!("mixless-{channel}-latest.json");
        if channel == "stable" {
            // `releases/latest` is GitHub's pinned redirect to the newest
            // non-prerelease; betas must be found through the API instead.
            return get_json(&format!("{repo}/releases/latest/download/{name}"));
        }
        let path = repo
            .strip_prefix("https://github.com/")
            .ok_or_else(|| format!("Unsupported repository URL: {repo}"))?;
        let releases: Vec<Release> = get_json(&format!(
            "https://api.github.com/repos/{path}/releases?per_page=30"
        ))?;
        let url = beta_manifest_url(&releases, &name)
            .ok_or("No beta release manifest found")?;
        get_json(&url)
    }

    /// Newest prerelease carrying the channel manifest; releases are sorted
    /// newest-first, so the first match wins. Drafts and partial uploads are
    /// skipped.
    fn beta_manifest_url<'a>(releases: &'a [Release], name: &str) -> Option<&'a str> {
        releases
            .iter()
            .filter(|r| r.prerelease && !r.draft)
            .find_map(|r| r.assets.iter().find(|a| a.name == name))
            .map(|a| a.browser_download_url.as_str())
    }

    fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T, String> {
        client(60)
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json::<T>())
            .map_err(|e| format!("GET {url}: {e}"))
    }

    fn client(timeout_secs: u64) -> reqwest::blocking::Client {
        reqwest::blocking::Client::builder()
            .user_agent(format!("mixless/{}", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .unwrap_or_else(|_| reqwest::blocking::Client::new())
    }

    fn download(url: &str, dest: &Path, progress: impl Fn(u8)) -> Result<(), String> {
        let mut response = client(900)
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Download failed: {e}"))?;
        let total = response.content_length().unwrap_or(0);
        let mut file = fs::File::create(dest)
            .map_err(|e| format!("Cannot write {}: {e}", dest.display()))?;
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
                progress((100 * written / total).min(100) as u8);
            }
        }
        if total > 0 && written != total {
            return Err("Download truncated".into());
        }
        Ok(())
    }

    fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
        let mut file = fs::File::open(path)
            .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
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

    struct Mount(PathBuf);

    impl Mount {
        fn attach(dmg: &Path, mountpoint: &Path) -> Result<Self, String> {
            fs::create_dir_all(mountpoint)
                .map_err(|e| format!("Cannot create mount point: {e}"))?;
            let status = Command::new("/usr/bin/hdiutil")
                .args(["attach", "-nobrowse", "-readonly", "-mountpoint"])
                .arg(mountpoint)
                .arg(dmg)
                .status()
                .map_err(|e| format!("Cannot run hdiutil: {e}"))?;
            if !status.success() {
                return Err("Cannot mount update DMG".into());
            }
            Ok(Self(mountpoint.to_path_buf()))
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Mount {
        fn drop(&mut self) {
            let _ = Command::new("/usr/bin/hdiutil")
                .args(["detach", "-quiet"])
                .arg(&self.0)
                .status();
        }
    }

    struct WorkDir(PathBuf);

    impl WorkDir {
        fn create(path: &Path) -> Result<Self, String> {
            fs::create_dir_all(path).map_err(|e| format!("Cannot create workspace: {e}"))?;
            Ok(Self(path.to_path_buf()))
        }
    }

    impl Drop for WorkDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn find_app(root: &Path) -> Result<PathBuf, String> {
        for entry in fs::read_dir(root).map_err(|e| format!("Cannot read DMG: {e}"))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.extension().and_then(|e| e.to_str()) == Some("app") {
                return Ok(path);
            }
        }
        Err("DMG contains no app".into())
    }

    fn update_target() -> Result<PathBuf, String> {
        if let Some(dir) = env::var_os("MIXLESS_UPDATE_TARGET") {
            return Ok(dir.into());
        }
        crate::self_install::current_bundle()
            .ok_or_else(|| "Cannot locate the installed Mixless.app".into())
    }

    /// Signed, notarized, from our developer, and actually the version the
    /// manifest promised — all four before anything is installed.
    fn verify_bundle(app: &Path, expected_version: &str) -> Result<(), String> {
        let status = Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(app)
            .status()
            .map_err(|e| format!("Cannot run codesign: {e}"))?;
        if !status.success() {
            return Err("Update signature verification failed".into());
        }
        let status = Command::new("/usr/bin/spctl")
            .args(["--assess", "--type", "execute"])
            .arg(app)
            .status()
            .map_err(|e| format!("Cannot run spctl: {e}"))?;
        if !status.success() {
            return Err("Update is not notarized".into());
        }
        let ours = crate::self_install::current_bundle()
            .ok_or("Cannot locate the running app bundle")?;
        let fresh = codesign_metadata(app)?;
        let installed = codesign_metadata(&ours)?;
        for key in ["Identifier", "TeamIdentifier"] {
            match (fresh.get(key), installed.get(key)) {
                (Some(a), Some(b)) if a == b => {}
                _ => return Err(format!("Update {key} mismatch")),
            }
        }
        let version = Command::new("/usr/bin/defaults")
            .arg("read")
            .arg(app.join("Contents/Info"))
            .arg("CFBundleShortVersionString")
            .output()
            .map_err(|e| format!("Cannot read update version: {e}"))?;
        if !version.status.success() {
            return Err("Cannot read update version".into());
        }
        if String::from_utf8_lossy(&version.stdout).trim() != expected_version {
            return Err("Update bundle version does not match the manifest".into());
        }
        Ok(())
    }

    fn codesign_metadata(app: &Path) -> Result<HashMap<String, String>, String> {
        let output = Command::new("/usr/bin/codesign")
            .args(["-dv", "--verbose=4"])
            .arg(app)
            .output()
            .map_err(|e| format!("Cannot run codesign: {e}"))?;
        if !output.status.success() {
            return Err("Unsigned app bundle".into());
        }
        Ok(String::from_utf8_lossy(&output.stderr)
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect())
    }

    #[cfg(test)]
    mod tests {
        use super::super::Asset;
        use super::*;

        fn release(draft: bool, prerelease: bool, assets: &[&str]) -> Release {
            Release {
                draft,
                prerelease,
                assets: assets
                    .iter()
                    .map(|name| Asset {
                        name: name.to_string(),
                        browser_download_url: format!("https://example.test/{name}"),
                    })
                    .collect(),
            }
        }

        #[test]
        fn beta_manifest_url_picks_newest_prerelease_with_manifest() {
            let releases = vec![
                release(false, false, &["mixless-stable-latest.json"]),
                release(false, true, &[]),
                release(false, true, &["mixless-beta-latest.json", "Mixless.dmg"]),
                release(false, true, &["mixless-beta-latest.json"]),
            ];
            let url = beta_manifest_url(&releases, "mixless-beta-latest.json");
            assert_eq!(url, Some("https://example.test/mixless-beta-latest.json"));
            assert!(beta_manifest_url(&releases, "mixless-missing.json").is_none());
            assert!(
                beta_manifest_url(&[release(true, true, &["mixless-beta-latest.json"])], "m")
                    .is_none()
            );
        }

        #[test]
        fn manifest_parses_release_feed_shape() {
            let json = serde_json::json!({
                "channel": "beta",
                "version": "0.1.0-beta.1",
                "tag": "v0.1.0-beta.1",
                "pub_date": "2026-09-16T05:42:17Z",
                "platforms": {
                    "darwin-universal": {
                        "url": "https://example.test/Mixless.dmg",
                        "sha256": "abc123",
                        "name": "Mixless.dmg"
                    }
                }
            });
            let manifest: Manifest = serde_json::from_value(json).unwrap();
            assert_eq!(manifest.channel, "beta");
            assert_eq!(
                manifest.platforms["darwin-universal"].sha256,
                "abc123"
            );
        }

        #[test]
        fn sha256_verification_matches_and_rejects() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("f.bin");
            std::fs::write(&path, b"payload").unwrap();
            let good = format!(
                "{:x}",
                Sha256::new().chain_update(b"payload").finalize()
            );
            verify_sha256(&path, &good).unwrap();
            verify_sha256(&path, &good.to_uppercase()).unwrap();
            assert!(verify_sha256(&path, &"0".repeat(64)).is_err());
        }

        #[test]
        fn find_app_requires_an_app_bundle() {
            let dir = tempfile::tempdir().unwrap();
            assert!(find_app(dir.path()).is_err());
            std::fs::create_dir(dir.path().join("Mixless.app")).unwrap();
            std::fs::write(dir.path().join("Applications"), b"").unwrap();
            assert_eq!(find_app(dir.path()).unwrap(), dir.path().join("Mixless.app"));
        }
    }
}
