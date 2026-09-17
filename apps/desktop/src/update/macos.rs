//! macOS package verification and installation.
use super::{Status, feed, network};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

pub(super) fn update(report: &dyn Fn(Status)) -> Result<Status, String> {
    let channel = env!("MIXLESS_CHANNEL");
    if channel == "dev" {
        return Err("Update checks require a signed release build".into());
    }
    let ours =
        crate::self_install::current_bundle().ok_or("Cannot locate the installed Mixless.app")?;
    if bundle_value(&ours, "MixlessReleaseChannel")? != channel {
        return Err("Installed bundle channel does not match this build".into());
    }
    let manifest = feed::manifest(channel)?;
    if !manifest.is_newer(channel, env!("CARGO_PKG_VERSION"))? {
        return Ok(Status::UpToDate);
    }
    // Another process may already have replaced the bundle while this older
    // executable is still running. Compare with the on-disk version as well.
    let installed = bundle_value(&ours, "CFBundleShortVersionString")?;
    if !manifest.is_newer(channel, &installed)? {
        return Ok(Status::Ready { version: installed });
    }
    install(&manifest, &ours, &update_target(&ours), report)
}

fn install(
    manifest: &feed::Manifest,
    ours: &Path,
    dest: &Path,
    report: &dyn Fn(Status),
) -> Result<Status, String> {
    let platform = manifest.platform()?;
    let work = tempfile::Builder::new()
        .prefix("mixless-update-")
        .tempdir()
        .map_err(|e| format!("Cannot create update workspace: {e}"))?;
    let dmg = work.path().join("update.dmg");
    network::download(&platform.url, &dmg, |percent| {
        report(Status::Downloading {
            version: manifest.version.clone(),
            percent,
        })
    })?;
    network::verify_sha256(&dmg, &platform.sha256)?;
    let mount = Mount::attach(&dmg, &work.path().join("mnt"))?;
    let app = find_app(mount.path())?;
    verify_bundle(&app, ours, &manifest.version, &manifest.channel)?;
    report(Status::Installing {
        version: manifest.version.clone(),
    });
    let mut ready_version = manifest.version.clone();
    crate::self_install::install_bundle_checked(&app, dest, |staged| {
        // Verify the copied bundle, too, before the atomic activation.
        verify_bundle(staged, ours, &manifest.version, &manifest.channel)
            .map_err(std::io::Error::other)?;
        // A different app process could have finished a newer update during
        // this download. Recheck under the install lock before activation.
        if dest.is_dir() {
            let channel =
                bundle_value(dest, "MixlessReleaseChannel").map_err(std::io::Error::other)?;
            let installed =
                bundle_value(dest, "CFBundleShortVersionString").map_err(std::io::Error::other)?;
            if !manifest
                .is_newer(&channel, &installed)
                .map_err(std::io::Error::other)?
            {
                ready_version = installed;
                return Ok(false);
            }
        }
        Ok(true)
    })
    .map_err(|e| format!("Install failed: {e}"))?;
    Ok(Status::Ready {
        version: ready_version,
    })
}

struct Mount(PathBuf);

impl Mount {
    fn attach(dmg: &Path, mountpoint: &Path) -> Result<Self, String> {
        fs::create_dir_all(mountpoint).map_err(|e| format!("Cannot create mount point: {e}"))?;
        let status = Command::new("/usr/bin/hdiutil")
            .args([
                "attach",
                "-nobrowse",
                "-noautoopen",
                "-readonly",
                "-mountpoint",
            ])
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
        let detached = Command::new("/usr/bin/hdiutil")
            .args(["detach", "-quiet"])
            .arg(&self.0)
            .status()
            .is_ok_and(|s| s.success());
        if !detached {
            // This mount belongs only to this worker; nothing is launched from it.
            let _ = Command::new("/usr/bin/hdiutil")
                .args(["detach", "-quiet", "-force"])
                .arg(&self.0)
                .status();
        }
    }
}

fn find_app(root: &Path) -> Result<PathBuf, String> {
    let mut apps = Vec::new();
    for entry in fs::read_dir(root).map_err(|e| format!("Cannot read DMG: {e}"))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().and_then(|e| e.to_str()) == Some("app") {
            if !entry.file_type().map_err(|e| e.to_string())?.is_dir() {
                return Err("DMG app must be a real bundle directory".into());
            }
            apps.push(entry.path());
        }
    }
    if apps.len() != 1 {
        return Err("DMG must contain exactly one app".into());
    }
    Ok(apps.remove(0))
}

fn update_target(ours: &Path) -> PathBuf {
    if let Some(dir) = env::var_os("MIXLESS_UPDATE_TARGET") {
        return dir.into();
    }
    ours.to_path_buf()
}

/// Signed, notarized, from our developer, and actually the version the
/// manifest promised — all four before anything is installed.
fn verify_bundle(
    app: &Path,
    ours: &Path,
    expected_version: &str,
    channel: &str,
) -> Result<(), String> {
    let status = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(app)
        .status()
        .map_err(|e| format!("Cannot run codesign: {e}"))?;
    if !status.success() {
        return Err("Update signature verification failed".into());
    }
    let status = Command::new("/usr/sbin/spctl")
        .args(["--assess", "--type", "execute"])
        .arg(app)
        .status()
        .map_err(|e| format!("Cannot run spctl: {e}"))?;
    if !status.success() {
        return Err("Update is not notarized".into());
    }
    let fresh = codesign_metadata(app)?;
    let installed = codesign_metadata(ours)?;
    for key in ["Identifier", "TeamIdentifier"] {
        match (fresh.get(key), installed.get(key)) {
            (Some(a), Some(b)) if a == b => {}
            _ => return Err(format!("Update {key} mismatch")),
        }
    }
    if bundle_value(app, "CFBundleShortVersionString")? != expected_version {
        return Err("Update bundle version does not match the manifest".into());
    }
    if bundle_value(app, "MixlessReleaseChannel")? != channel {
        return Err("Update bundle channel does not match the manifest".into());
    }
    Ok(())
}

fn bundle_value(app: &Path, key: &str) -> Result<String, String> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-extract", key, "raw", "-o", "-"])
        .arg(app.join("Contents/Info.plist"))
        .output()
        .map_err(|e| format!("Cannot read bundle {key}: {e}"))?;
    if !output.status.success() {
        return Err(format!("Cannot read bundle {key}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
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
#[path = "macos_tests.rs"]
mod tests;
