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
        .unwrap()
        .to_path_buf();
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("bundle") => bundle(&root, &args[1..]),
        Some("set-version") => set_version(&root, args.get(1).map(String::as_str)),
        _ => Err(
            "Usage: cargo run -p mixless-tools -- bundle [BINARY OUTPUT.app] [--version V] [--build-number N] [--channel C]\n       cargo run -p mixless-tools -- set-version SEMVER"
                .into(),
        ),
    }
}

fn semver(version: &str) -> bool {
    let (base, prerelease) = version.split_once('-').unwrap_or((version, ""));
    let parts: Vec<_> = base.split('.').collect();
    let base_ok = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    let pre_ok = prerelease.is_empty()
        || prerelease
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    base_ok && pre_ok
}

fn option<'a>(
    args: &'a [String],
    name: &str,
) -> Result<Option<&'a str>, Box<dyn std::error::Error>> {
    let Some(index) = args.iter().position(|arg| arg == name) else {
        return Ok(None);
    };
    args.get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("{name} requires a value").into())
        .map(Some)
}

fn bundle(root: &Path, args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let positional: Vec<_> = args
        .iter()
        .take_while(|arg| !arg.starts_with("--"))
        .collect();
    let binary = positional
        .first()
        .map(|path| PathBuf::from(path.as_str()))
        .unwrap_or_else(|| root.join("target/debug/mixless"));
    let output = positional
        .get(1)
        .map(|path| PathBuf::from(path.as_str()))
        .unwrap_or_else(|| root.join("target/app/Mixless.app"));
    let version = option(args, "--version")?
        .unwrap_or(env!("CARGO_PKG_VERSION"))
        .to_owned();
    if !semver(&version) {
        return Err(format!("--version must be a semantic version, got {version:?}").into());
    }
    let build_number = option(args, "--build-number")?.unwrap_or("1").to_owned();
    // CFBundleVersion allows up to three dot-separated positive integers.
    let build_parts: Vec<_> = build_number.split('.').collect();
    if build_parts.len() > 3
        || build_parts
            .iter()
            .any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(format!(
            "--build-number must be up to three dot-separated integers, got {build_number:?}"
        )
        .into());
    }
    let channel = option(args, "--channel")?.unwrap_or("dev").to_owned();
    if !["stable", "beta", "dev"].contains(&channel.as_str()) {
        return Err(format!("--channel must be stable, beta or dev, got {channel:?}").into());
    }
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
<key>CFBundleVersion</key><string>{build_number}</string>
<key>CFBundleSupportedPlatforms</key><array><string>MacOSX</string></array>
<key>LSApplicationCategoryType</key><string>public.app-category.music</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>MixlessReleaseChannel</key><string>{channel}</string>
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
    println!(
        "App bundle ({channel} {version}, build {build_number}): {}",
        output.display()
    );
    Ok(())
}

fn set_version(root: &Path, version: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let version = version
        .map(|v| v.strip_prefix('v').unwrap_or(v))
        .filter(|v| semver(v))
        .ok_or("Usage: set-version SEMVER, for example 1.2.3 or 1.2.3-beta.1")?;
    let manifest = root.join("Cargo.toml");
    let text = fs::read_to_string(&manifest)?;
    let table = text
        .find("[workspace.package]")
        .ok_or("Cargo.toml has no [workspace.package] table")?;
    let table_end = text[table + 1..]
        .find("\n[")
        .map(|index| table + 1 + index)
        .unwrap_or(text.len());
    let rest = &text[table..table_end];
    let quote = rest
        .find("\nversion")
        .and_then(|index| rest[index..].find('"').map(|start| index + start))
        .map(|index| table + index)
        .ok_or("[workspace.package] has no version key")?;
    let tail = &text[quote + 1..];
    let end = tail.find('"').ok_or("unterminated workspace version")?;
    let updated = format!("{}\"{}\"{}", &text[..quote], version, &tail[end + 1..]);
    fs::write(&manifest, updated)?;
    println!("Workspace version set to {version}");
    Ok(())
}
