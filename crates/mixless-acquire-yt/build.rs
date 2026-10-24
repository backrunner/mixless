fn main() {
    let source = std::env::var("DEP_FFMPEG_SOURCE_ARCHIVE")
        .expect("the pinned FFmpeg build must supply its corresponding source archive");
    println!("cargo:rustc-env=MIXLESS_AUDIO_SOURCE_ARCHIVE={source}");
}
