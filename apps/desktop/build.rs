use std::process::Command;

fn main() {
    // Source edits and commits must both change the visible identity. Cargo's
    // normal package fingerprint handles source changes; watch Git explicitly.
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=resources");
    println!("cargo:rerun-if-changed=../../assets/branding");
    println!("cargo:rerun-if-changed=native/icon.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_icon_bridge();
    }
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=../../crates");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/refs/heads");
    println!("cargo:rerun-if-changed=../../.git/index");
    let revision =
        output("git", &["describe", "--always", "--dirty"]).unwrap_or_else(|| "source".into());
    let built =
        output("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]).unwrap_or_else(|| "unknown-time".into());
    println!("cargo:rustc-env=MIXLESS_REVISION={revision}");
    println!("cargo:rustc-env=MIXLESS_BUILD_TIME={built}");
    println!(
        "cargo:rustc-env=MIXLESS_BUILD_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );
}

fn build_icon_bridge() {
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let object = out.join("icon.o");
    let library = out.join("libmixless_icon.a");
    let target = std::env::var("TARGET").expect("TARGET");
    let status = Command::new("xcrun")
        .args([
            "clang",
            "-target",
            &target,
            "-fobjc-arc",
            "-c",
            "native/icon.m",
            "-o",
        ])
        .arg(&object)
        .status()
        .expect("compile AppKit icon bridge");
    assert!(status.success(), "AppKit icon bridge compilation failed");
    let status = Command::new("ar")
        .arg("crs")
        .arg(&library)
        .arg(&object)
        .status()
        .expect("archive AppKit icon bridge");
    assert!(status.success(), "AppKit icon bridge archive failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=mixless_icon");
    println!("cargo:rustc-link-lib=framework=AppKit");
}

fn output(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program).args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}
