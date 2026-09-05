use std::process::Command;

fn main() {
    // Source edits and commits must both change the visible identity. Cargo's
    // normal package fingerprint handles source changes; watch Git explicitly.
    println!("cargo:rerun-if-changed=src");
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

fn output(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program).args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}
