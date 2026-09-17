//! Stage and validate a complete bundle before atomically replacing it.
use std::{fs, io, path::Path};

pub(crate) fn install_bundle(source: &Path, dest: &Path) -> io::Result<()> {
    install_bundle_checked(source, dest, |_| Ok(true)).map(|_| ())
}

pub(crate) fn install_bundle_checked(
    source: &Path,
    dest: &Path,
    check: impl FnOnce(&Path) -> io::Result<bool>,
) -> io::Result<bool> {
    let parent = dest
        .parent()
        .filter(|p| p.is_dir())
        .ok_or_else(|| io::Error::other("Install directory does not exist"))?;
    let name = dest
        .file_name()
        .ok_or_else(|| io::Error::other("Missing bundle name"))?;
    if !source.is_dir() || dest.extension().and_then(|s| s.to_str()) != Some("app") {
        return Err(io::Error::other("Expected an app bundle destination"));
    }
    // Keep this inode in place: unlinking a lock file allows a second worker
    // to lock a new inode while a waiter still holds the old one. The OS
    // releases the lock on failure, process exit, or a crash.
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(parent.join(format!(".{}.install.lock", name.to_string_lossy())))?;
    lock.try_lock()
        .map_err(|e| io::Error::other(format!("Cannot lock app installation: {e}")))?;
    let replaced = match fs::symlink_metadata(dest) {
        Ok(meta) if meta.file_type().is_dir() => true,
        Ok(_) => {
            return Err(io::Error::other(
                "Install destination is not a real bundle directory",
            ));
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => false,
        Err(e) => return Err(e),
    };
    // Same volume as the destination, private, and unique across processes.
    // Copy/verification failure leaves the old application untouched.
    let work = tempfile::Builder::new()
        .prefix(".mixless-install-")
        .tempdir_in(parent)?;
    let staged = work.path().join(name);
    copy_tree(source, &staged)?;
    if !check(&staged)? {
        return Ok(false);
    }
    activate(&staged, dest, replaced)?;
    // On replacement, the old bundle is now at `staged`; the temp directory
    // removes it only after the new bundle is atomically visible at `dest`.
    Ok(true)
}

#[cfg(target_os = "macos")]
fn activate(staged: &Path, dest: &Path, replaced: bool) -> io::Result<()> {
    use std::{
        ffi::CString,
        os::raw::{c_char, c_int, c_uint},
        os::unix::ffi::OsStrExt,
    };
    unsafe extern "C" {
        fn renamex_np(from: *const c_char, to: *const c_char, flags: c_uint) -> c_int;
    }
    let from = CString::new(staged.as_os_str().as_bytes())?;
    let to = CString::new(dest.as_os_str().as_bytes())?;
    // Darwin RENAME_SWAP / RENAME_EXCL: no interval with a missing or partly
    // copied app, and never silently overwrite a concurrently created app.
    let flags = if replaced { 0x0000_0002 } else { 0x0000_0004 };
    // SAFETY: Both C strings are valid for the duration of this syscall.
    if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), flags) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn activate(staged: &Path, dest: &Path, replaced: bool) -> io::Result<()> {
    if replaced {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Atomic app replacement requires macOS",
        ));
    }
    fs::rename(staged, dest)
}

fn copy_tree(source: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir(dest)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = dest.join(entry.file_name());
        if kind.is_symlink() {
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)?;
            fs::File::open(&target)?.sync_all()?;
        } else {
            return Err(io::Error::other("Unsupported file in application bundle"));
        }
    }
    fs::set_permissions(dest, fs::metadata(source)?.permissions())?;
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::{PermissionsExt, symlink},
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };

    fn bundle(root: &Path, name: &str, contents: &[u8]) -> std::path::PathBuf {
        let app = root.join(name);
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        let binary = app.join("Contents/MacOS/mixless");
        fs::write(&binary, contents).unwrap();
        fs::set_permissions(binary, fs::Permissions::from_mode(0o755)).unwrap();
        symlink("MacOS/mixless", app.join("Contents/link")).unwrap();
        app
    }

    #[test]
    fn failed_staging_and_verification_leave_the_original_app_intact() {
        let dir = tempfile::tempdir().unwrap();
        let dest = bundle(dir.path(), "Installed.app", b"old");
        assert!(install_bundle(&dir.path().join("missing.app"), &dest).is_err());
        let source = bundle(dir.path(), "Source.app", b"new");
        let socket_path = source.join("Contents/s");
        let _socket = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
        assert!(install_bundle(&source, &dest).is_err());
        fs::remove_file(socket_path).unwrap();
        assert!(
            install_bundle_checked(&source, &dest, |_| Err(io::Error::other("bad signature")))
                .is_err()
        );
        assert_eq!(
            fs::read(dest.join("Contents/MacOS/mixless")).unwrap(),
            b"old"
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn replacement_is_always_complete_and_preserves_executables_and_links() {
        let dir = tempfile::tempdir().unwrap();
        let dest = bundle(dir.path(), "Installed.app", b"old");
        let source = bundle(dir.path(), "Source.app", b"new");
        let finished = Arc::new(AtomicBool::new(false));
        let reader_done = finished.clone();
        let binary = dest.join("Contents/MacOS/mixless");
        let reader = std::thread::spawn(move || {
            while !reader_done.load(Ordering::Acquire) {
                let contents = fs::read(&binary).unwrap();
                assert!(contents == b"old" || contents == b"new");
            }
        });
        install_bundle_checked(&source, &dest, |staged| {
            assert_eq!(fs::read(dest.join("Contents/link"))?, b"old");
            assert_eq!(fs::read(staged.join("Contents/link"))?, b"new");
            Ok(true)
        })
        .unwrap();
        finished.store(true, Ordering::Release);
        reader.join().unwrap();
        assert_eq!(fs::read(dest.join("Contents/link")).unwrap(), b"new");
        assert_eq!(
            fs::metadata(dest.join("Contents/MacOS/mixless"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
        let fresh = dir.path().join("New.app");
        install_bundle(&source, &fresh).unwrap();
        assert_eq!(fs::read(fresh.join("Contents/link")).unwrap(), b"new");
    }

    #[test]
    fn concurrent_installer_cannot_replace_the_same_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let dest = bundle(dir.path(), "Installed.app", b"old");
        let source = bundle(dir.path(), "Source.app", b"new");
        let installed = install_bundle_checked(&source, &dest, |_| {
            assert!(install_bundle(&source, &dest).is_err());
            Ok(false) // a final version check can discard the staged copy
        })
        .unwrap();
        assert!(!installed);
        assert_eq!(fs::read(dest.join("Contents/link")).unwrap(), b"old");
        install_bundle(&source, &dest).unwrap(); // failure/no-op released the lock
    }
}
