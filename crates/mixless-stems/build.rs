fn main() {
    println!("cargo:rustc-check-cfg=cfg(stems_ort)");
    let intel_macos = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("x86_64");
    if !intel_macos {
        println!("cargo:rustc-cfg=stems_ort");
    }
}
