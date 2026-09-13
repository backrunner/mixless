//! Small native packaging tool. No scripting-language runtime is required.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

fn atomic_copy(source: &Path, dest: &Path) -> std::io::Result<()> {
    let temp = dest.with_extension(format!("{}.next", std::process::id()));
    fs::copy(source, &temp)?;
    fs::File::open(&temp)?.sync_all()?;
    fs::rename(temp, dest)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) != Some("bundle") {
        return Err("Usage: cargo run -p mixless-tools -- bundle [BINARY OUTPUT.app]".into());
    }
    let binary = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/debug/mixless"));
    let output = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/app/Mixless.app"));
    if !binary.is_file() || output.extension().and_then(|s| s.to_str()) != Some("app") {
        return Err("Expected a built binary and an .app output path".into());
    }
    let contents = output.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    // Atomic replacement preserves an already-running executable's inode.
    atomic_copy(&binary, &macos.join("mixless"))?;
    atomic_copy(
        &root.join("apps/desktop/resources/Mixless.icns"),
        &resources.join("Mixless.icns"),
    )?;
    atomic_copy(
        &root.join("THIRD_PARTY_NOTICES.md"),
        &resources.join("THIRD_PARTY_NOTICES.md"),
    )?;
    let licenses = root.join("third-party");
    if licenses.exists() {
        let dest = resources.join("Licenses");
        fs::create_dir_all(&dest)?;
        for file in fs::read_dir(licenses)?.flatten() {
            if file.path().is_file() {
                atomic_copy(&file.path(), &dest.join(file.file_name()))?;
            }
        }
    }
    let version = env!("CARGO_PKG_VERSION");
    let info = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Mixless</string>
<key>CFBundleDisplayName</key><string>Mixless</string>
<key>CFBundleIdentifier</key><string>app.mixless.desktop</string>
<key>CFBundleExecutable</key><string>mixless</string>
<key>CFBundleIconFile</key><string>Mixless.icns</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>{version}</string>
<key>CFBundleVersion</key><string>1</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSSupportsAutomaticGraphicsSwitching</key><true/>
<key>NSHumanReadableCopyright</key><string>Mixless contributors. MPL-2.0.</string>
</dict></plist>
"#
    );
    let temp = contents.join("Info.plist.next");
    let mut file = fs::File::create(&temp)?;
    file.write_all(info.as_bytes())?;
    file.sync_all()?;
    fs::rename(temp, contents.join("Info.plist"))?;
    println!("Local unsigned app: {}", output.display());
    Ok(())
}
