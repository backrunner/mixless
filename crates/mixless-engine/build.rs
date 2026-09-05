fn main() {
    println!("cargo:rerun-if-changed=native");
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .opt_level(2)
        .include("native/vendor")
        .file("native/stretch.cpp");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build.define("SIGNALSMITH_USE_ACCELERATE", None);
        println!("cargo:rustc-link-lib=framework=Accelerate");
    }
    build.compile("mixless_stretch");
}
