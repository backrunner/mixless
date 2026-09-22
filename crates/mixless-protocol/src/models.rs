//! Pinned model identities shared by runtime inference and release packaging.
pub struct ModelArtifact {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub max_size: u64,
}

pub const STEM_SEPARATOR: ModelArtifact = ModelArtifact {
    name: "htdemucs-fp16.onnx",
    url: "https://huggingface.co/StemSplitio/htdemucs-onnx/resolve/d54ed9eb60e258ea82131c6ee14578628816456a/htdemucs_fp16weights.onnx",
    sha256: "d05c269d0178d2a72ad484b10b11dd370193fc923201c3b27a99f848745db70a",
    max_size: 165_612_636,
};
pub const STEM_NOTES: ModelArtifact = ModelArtifact {
    name: "basic-pitch.onnx",
    url: "https://raw.githubusercontent.com/spotify/basic-pitch/v0.4.0/basic_pitch/saved_models/icassp_2022/nmp.onnx",
    sha256: "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec",
    max_size: 20_000_000,
};
pub const STEM_MODELS: [ModelArtifact; 2] = [STEM_SEPARATOR, STEM_NOTES];
