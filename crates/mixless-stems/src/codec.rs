//! Encoders run on cache workers. All formats decode to aligned f32 before playback.
mod decode;
#[cfg(any(stems_ort, test))]
mod encode;
#[cfg(any(stems_ort, test))]
mod flac;
#[cfg(test)]
mod tests;
pub(crate) use decode::{probe, read};
#[cfg(any(stems_ort, test))]
pub(crate) use encode::write;

pub(crate) fn codec_error(error: impl std::fmt::Display) -> crate::Error {
    crate::Error::Model(format!("Stem codec: {error}"))
}
