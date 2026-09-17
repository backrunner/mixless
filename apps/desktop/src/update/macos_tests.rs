use super::*;

#[test]
fn disk_image_must_contain_one_real_app() {
    let dir = tempfile::tempdir().unwrap();
    assert!(find_app(dir.path()).is_err());
    let app = dir.path().join("Mixless.app");
    fs::create_dir(&app).unwrap();
    assert_eq!(find_app(dir.path()).unwrap(), app);
    fs::create_dir(dir.path().join("Other.app")).unwrap();
    assert!(find_app(dir.path()).is_err());
    let linked = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(&app, linked.path().join("Linked.app")).unwrap();
    assert!(find_app(linked.path()).is_err());
}

/// Downloads the public release specified by the locally supplied manifest.
/// All writes and process launches stay inside this test's private directory.
#[test]
#[ignore = "Requires MIXLESS_TEST_UPDATE_MANIFEST pointing at a real signed release manifest; uses network and Gatekeeper"]
fn published_package_roundtrip() {
    let path = env::var_os("MIXLESS_TEST_UPDATE_MANIFEST").expect("manifest path");
    let manifest: feed::Manifest = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    let live = feed::manifest(&manifest.channel).unwrap();
    assert_eq!(
        live.version, manifest.version,
        "Release feed changed during this audit"
    );
    assert_eq!(
        live.platform().unwrap().sha256,
        manifest.platform().unwrap().sha256
    );
    let work = tempfile::tempdir().unwrap();
    let dmg = work.path().join("release.dmg");
    let platform = manifest.platform().unwrap();
    network::download(&platform.url, &dmg, |_| {}).unwrap();
    network::verify_sha256(&dmg, &platform.sha256).unwrap();
    let mounted = Mount::attach(&dmg, &work.path().join("mount")).unwrap();
    let source = find_app(mounted.path()).unwrap();
    verify_bundle(&source, &source, &manifest.version, &manifest.channel).unwrap();
    assert!(verify_bundle(&source, &source, "99.0.0", &manifest.channel).is_err());
    assert!(verify_bundle(&source, &source, &manifest.version, "wrong-channel").is_err());
    let dest = work.path().join("Mixless.app");
    crate::self_install::install_bundle(&source, &dest).unwrap();
    // Reinstall the real signed bundle through the same checked atomic
    // activation primitive. Production version policy deliberately skips
    // reinstalling an equal version; that no-op is exercised just below.
    assert!(
        crate::self_install::install_bundle_checked(&source, &dest, |staged| {
            verify_bundle(staged, &source, &manifest.version, &manifest.channel)
                .map_err(std::io::Error::other)?;
            Ok(true)
        })
        .unwrap()
    );
    let states = std::cell::RefCell::new(Vec::new());
    use std::os::unix::fs::MetadataExt;
    let installed_inode = fs::metadata(&dest).unwrap().ino();
    let result = install(&manifest, &dest, &dest, &|s| states.borrow_mut().push(s)).unwrap();
    assert_eq!(
        fs::metadata(&dest).unwrap().ino(),
        installed_inode,
        "An equal version must not replace the installed bundle again"
    );
    assert_eq!(
        result,
        Status::Ready {
            version: manifest.version.clone()
        }
    );
    assert!(
        states
            .borrow()
            .iter()
            .any(|s| matches!(s, Status::Downloading { percent: 100, .. }))
    );
    assert!(
        states
            .borrow()
            .iter()
            .any(|s| matches!(s, Status::Installing { .. }))
    );
    verify_bundle(&dest, &source, &manifest.version, &manifest.channel).unwrap();
    assert!(
        Command::new("/usr/bin/xcrun")
            .args(["stapler", "validate"])
            .arg(&dest)
            .status()
            .unwrap()
            .success()
    );
    // Restart smoke: the actual shipped executable reaches its early install
    // entry point and exits; no user library, running DJ session or UI is touched.
    assert!(
        Command::new(dest.join("Contents/MacOS/mixless"))
            .env("MIXLESS_INSTALL_ONLY", "1")
            .env("MIXLESS_DATA_DIR", work.path().join("data"))
            .env("MIXLESS_OFFLINE", "1")
            .env("MIXLESS_MODELS_OFFLINE", "1")
            .status()
            .unwrap()
            .success()
    );
    eprintln!(
        "Signed release {}: download, hash, mount, identity/channel/version, atomic replacement, Gatekeeper, stapler and executable restart passed",
        manifest.version
    );
}
