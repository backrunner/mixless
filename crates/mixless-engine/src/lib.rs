//! Realtime dual-deck mixer. Host must not put DSP in src-tauri.

mod decode;
mod device;
mod dsp;
mod effects;
mod engine;
mod loudness;
mod master;
pub use master::master_eq_response;
mod resample;
mod source;
pub use source::StemBuffer;
mod stretch;
mod waveform;

pub use decode::{AudioBuffer, decode_file};
pub use device::{AudioConfig, AudioDevice, AudioOutputFormat, DeviceError, output_devices};
pub use engine::{Engine, EngineConfig, EngineError, PreparedMix, RecordingStatus};
pub use loudness::Loudness;
pub use mixless_protocol as proto;
pub use waveform::{compute_preview_waveform, compute_waveform};
