//! Pinned model identities shared by runtime inference and release packaging.
#[derive(Clone, Copy)]
pub struct ModelArtifact {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub max_size: u64,
}

/// Single BS-RoFormer graph, four jointly predicted stems in drums/bass/other/vocals order.
pub const STEM_SEPARATOR: ModelArtifact = ModelArtifact {
    name: "bs_roformer_4stem_rhythm_fp16.onnx",
    url: "https://huggingface.co/silverdaw/bs-roformer-rhythm-onnx/resolve/9fa8c3acdbd488b97309e4b78c9c605389cec777/bs_roformer_4stem_rhythm_fp16.onnx",
    sha256: "4bac38e42c085b5d69964e2b7ff161318a1315122c10b025c718b20e94d85d6b",
    max_size: 269_457_840,
};
pub const STEM_SEPARATOR_HASH: &str = STEM_SEPARATOR.sha256;
pub const STEM_SEPARATORS: [ModelArtifact; 1] = [STEM_SEPARATOR];
pub const STEM_NOTES: ModelArtifact = ModelArtifact {
    name: "basic-pitch.onnx",
    url: "https://raw.githubusercontent.com/spotify/basic-pitch/v0.4.0/basic_pitch/saved_models/icassp_2022/nmp.onnx",
    sha256: "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec",
    max_size: 20_000_000,
};
pub const STEM_MODELS: [ModelArtifact; 2] = [STEM_SEPARATOR, STEM_NOTES];
