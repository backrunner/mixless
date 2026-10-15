//! On-disk representation, independent of inference/evidence version.
use serde::{Deserialize, Serialize};
#[cfg(all(test, stems_ort))]
mod tests;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFormat {
    #[default]
    Wav,
    Flac,
    Mp3,
}

impl StorageFormat {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Mp3 => "mp3",
        }
    }
}

pub(crate) const NAMES: [&str; 4] = ["vocals", "drums", "instruments", "bass"];
pub(crate) const RATE: u32 = 44_100;
pub(crate) const MAX_FRAMES: u64 = RATE as u64 * 20 * 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Storage {
    pub version: u32,
    pub format: StorageFormat,
    pub frames: u64,
    /// Restore headroom after encoding integer FLAC or normalized MP3 input.
    pub gain: [f32; 4],
    pub bytes: [u64; 4],
    pub blake3: [String; 4],
}

impl Storage {
    pub fn valid(&self, duration: f32) -> bool {
        self.version == 1
            && self.frames > 0
            && self.frames <= MAX_FRAMES
            && (self.frames as f64 / RATE as f64 - duration as f64).abs() <= 0.05
            && self
                .gain
                .iter()
                .all(|g| g.is_finite() && (1.0..=128.).contains(g))
            && self
                .bytes
                .iter()
                .all(|&b| b > 0 && b <= MAX_FRAMES * 8 + 65536)
            && self
                .blake3
                .iter()
                .all(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Manifest {
    #[serde(flatten)]
    pub analysis: mixless_protocol::StemAnalysis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<Storage>,
}

impl Manifest {
    pub fn format(&self) -> StorageFormat {
        self.storage
            .as_ref()
            .map_or(StorageFormat::Wav, |s| s.format)
    }
    pub fn files(&self) -> [String; 5] {
        std::array::from_fn(|i| {
            if i == 0 {
                "analysis.json".into()
            } else {
                format!("{}.{}", NAMES[i - 1], self.format().extension())
            }
        })
    }
}
