fn main() {
    println!("cargo:rerun-if-changed=native/sound_analysis.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    cc::Build::new()
        .file("native/sound_analysis.m")
        .flag("-fobjc-arc")
        .flag("-O2")
        .compile("mixless_sound_analysis");
    for framework in ["Foundation", "AVFoundation", "SoundAnalysis", "CoreMedia"] {
        println!("cargo:rustc-link-lib=framework={framework}");
    }
}
