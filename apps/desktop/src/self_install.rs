//! Self-installing disk image launch: a build started from a mounted DMG (or
//! the App Translocation path Gatekeeper substitutes for it) copies itself
//! into /Applications and relaunches, so double-clicking Mixless.app inside
//! the disk image behaves like an installer. Launches from any other location
//! are left alone.
use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process::{self, Command},
};

pub fn relocate_from_disk_image() {
    let install_only = env::var_os("MIXLESS_INSTALL_ONLY").is_some();
    match relocate(install_only) {
        // Relocation ends the process via the relaunched copy.
        Ok(()) if install_only => process::exit(0),
        Ok(()) => {}
        Err(error) => {
            tracing::warn!(%error, "disk-image install failed; continuing in place");
            if install_only {
                process::exit(1);
            }
        }
    }
}

fn relocate(install_only: bool) -> io::Result<()> {
    let Some(bundle) = current_bundle() else {
        return Ok(());
    };
    let text = bundle.to_string_lossy();
    let home = dirs::home_dir().unwrap_or_default();
    if text.starts_with("/Applications/")
        || text.starts_with(&format!("{}/Applications/", home.display()))
    {
        return Ok(());
    }
    // Mounted disk images are read-only; App Translocation presents the
    // quarantined copy at a randomized read-only path. Any other launch
    // location (Downloads, an external drive) is left alone.
    let on_disk_image =
        text.contains("/AppTranslocation/") || !writable(bundle.parent().unwrap_or(&bundle));
    if !on_disk_image {
        return Ok(());
    }

    let dest = install_dir()?.join(bundle.file_name().unwrap_or_default());
    if dest == bundle {
        return Ok(());
    }
    tracing::info!(from = %bundle.display(), to = %dest.display(), "installing Mixless");
    install_bundle(&bundle, &dest)?;
    // The installed copy keeps the quarantine attribute; clearing it skips the
    // first-launch Gatekeeper dialog. Best effort: a notarized copy still
    // passes Gatekeeper if removal is denied.
    let _ = Command::new("/usr/bin/xattr")
        .args(["-dr", "com.apple.quarantine"])
        .arg(&dest)
        .status();
    if install_only {
        return Ok(());
    }
    match Command::new("/usr/bin/open").arg("-n").arg(&dest).status() {
        Ok(status) if status.success() => process::exit(0),
        other => tracing::warn!(?other, "installed but relaunch failed; continuing in place"),
    }
    Ok(())
}

pub(crate) fn current_bundle() -> Option<PathBuf> {
    let exe = env::current_exe().ok()?.canonicalize().ok()?;
    exe.ancestors()
        .find(|path| path.extension().and_then(|ext| ext.to_str()) == Some("app"))
        .map(Path::to_path_buf)
}

fn install_dir() -> io::Result<PathBuf> {
    if let Ok(dir) = env::var("MIXLESS_INSTALL_DIR") {
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir)?;
        return Ok(dir);
    }
    let system = Path::new("/Applications");
    if writable(system) {
        return Ok(system.to_path_buf());
    }
    let user = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Applications");
    fs::create_dir_all(&user)?;
    Ok(user)
}

fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".mixless-write-test-{}", process::id()));
    match fs::File::create(&probe) {
        Ok(_) => {
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub(crate) fn install_bundle(source: &Path, dest: &Path) -> io::Result<()> {
    let backup = dest.with_file_name(format!(
        ".{}-replaced-{}",
        dest.file_name().unwrap_or_default().to_string_lossy(),
        process::id()
    ));
    let replaced = dest.exists();
    if replaced {
        fs::rename(dest, &backup)?;
    }
    match copy_tree(source, dest) {
        Ok(()) => {
            if replaced {
                let _ = fs::remove_dir_all(&backup);
            }
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_dir_all(dest);
            if replaced {
                let _ = fs::rename(&backup, dest);
            }
            Err(error)
        }
    }
}

fn copy_tree(source: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}
