//! Realtime dual-deck mixer. Host must not put DSP in src-tauri.

mod decode;
mod device;
mod dsp;
mod effects;
mod engine;
mod resample;
mod stretch;
mod waveform;

pub use decode::{decode_file, AudioBuffer};
pub use device::{output_devices, AudioConfig, AudioDevice, DeviceError};
pub use engine::{Engine, EngineConfig, EngineError, PreparedMix};
pub use mixless_protocol as proto;
pub use waveform::compute_waveform;
