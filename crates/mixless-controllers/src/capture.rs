//! Input observations are separate from reviewed replay fixtures. No output API is used.
use crate::{error, hid_backend, MidiError};
use mixless_midi::midir;
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::{Duration, Instant},
};

const MAX_FRAMES: usize = 4096;
const MAX_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputCapture {
    pub version: u32,
    pub capture_type: String,
    pub protocol: String,
    pub model: String,
    pub firmware: String,
    pub os: String,
    pub truncated: bool,
    pub frames: Vec<InputFrame>,
    #[serde(skip)]
    bytes: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InputFrame {
    Input { at_us: u64, data: Vec<u8> },
    Marker { at_us: u64, label: String },
    Disconnect { at_us: u64 },
}
impl InputFrame {
    fn timestamp(&self) -> u64 {
        match self {
            Self::Input { at_us, .. } | Self::Marker { at_us, .. } | Self::Disconnect { at_us } => {
                *at_us
            }
        }
    }
}
impl InputCapture {
    pub fn new(protocol: &str, model: &str, firmware: &str, os: &str) -> Result<Self, MidiError> {
        if !matches!(protocol, "midi" | "hid")
            || [model, firmware, os]
                .iter()
                .any(|s| s.is_empty() || s.len() > 200)
        {
            return Err(error(
                "Specify midi/hid and model, firmware and OS (use unknown if necessary)",
            ));
        }
        Ok(Self {
            version: 1,
            capture_type: "input_observation".into(),
            protocol: protocol.into(),
            model: model.into(),
            firmware: firmware.into(),
            os: os.into(),
            truncated: false,
            frames: vec![],
            bytes: 0,
        })
    }
    pub fn push(&mut self, frame: InputFrame) {
        let size = match &frame {
            InputFrame::Input { data, .. } if data.is_empty() || data.len() > 4096 => {
                self.truncated = true;
                return;
            }
            InputFrame::Input { data, .. } => data.len(),
            InputFrame::Marker { label, .. } if label.is_empty() || label.len() > 200 => {
                self.truncated = true;
                return;
            }
            InputFrame::Marker { label, .. } => label.len(),
            InputFrame::Disconnect { .. } => 0,
        } + 64;
        if self.frames.len() >= MAX_FRAMES || self.bytes + size > MAX_BYTES {
            self.truncated = true;
            return;
        }
        // Markers and transport callbacks have separate producers; keep a single
        // monotonic observation order without changing the raw bytes.
        let index = self
            .frames
            .partition_point(|f| f.timestamp() <= frame.timestamp());
        self.frames.insert(index, frame);
        self.bytes += size;
    }
}

/// Opening a selected input can change OS device ownership. This function never
/// applies a preset, opens a MIDI output, writes HID reports or reads features.
pub fn capture_input(
    protocol: &str,
    device_id: &str,
    seconds: u64,
    model: &str,
    firmware: &str,
    os: &str,
    markers: mpsc::Receiver<String>,
) -> Result<InputCapture, MidiError> {
    if !(1..=600).contains(&seconds) {
        return Err(error("Capture duration must be 1–600 seconds"));
    }
    let mut capture = InputCapture::new(protocol, model, firmware, os)?;
    let start = Instant::now();
    let timestamp = || start.elapsed().as_micros() as u64;
    let dropped = Arc::new(AtomicBool::new(false));
    let (tx, rx) = mpsc::sync_channel(512);
    let mut midi_connection = None;
    let mut hid_device = None;
    if protocol == "midi" {
        let mut midi = midir::MidiInput::new("mixless-input-observation").map_err(error)?;
        midi.ignore(midir::Ignore::None);
        let port = midi
            .ports()
            .into_iter()
            .find(|p| p.id() == device_id)
            .ok_or_else(|| error("Selected MIDI input unavailable"))?;
        let overflow = dropped.clone();
        midi_connection = Some(
            midi.connect(
                &port,
                "mixless-input-observation",
                move |_, data, _| {
                    if data.len() > 4096 {
                        overflow.store(true, Ordering::Relaxed);
                        return;
                    }
                    if tx
                        .try_send(InputFrame::Input {
                            at_us: start.elapsed().as_micros() as u64,
                            data: data.into(),
                        })
                        .is_err()
                    {
                        overflow.store(true, Ordering::Relaxed);
                    }
                },
                (),
            )
            .map_err(error)?,
        );
    } else {
        let info = hid_backend::devices()
            .map_err(error)?
            .into_iter()
            .find(|info| hid_backend::identity(info) == device_id)
            .ok_or_else(|| error("Selected HID input unavailable"))?;
        hid_device = Some(
            hid_backend::request_open(info.path())
                .map_err(error)?
                .recv_timeout(Duration::from_secs(2))
                .map_err(error)?
                .map_err(error)?,
        );
    }
    let mut buffer = [0u8; 4096];
    while start.elapsed() < Duration::from_secs(seconds) && !capture.truncated {
        if let Some(device) = &hid_device {
            match device.device.read_timeout(&mut buffer, 10) {
                Ok(0) => {}
                Ok(n) => {
                    let mut data = Vec::with_capacity(n + 1);
                    if !device.numbered {
                        data.push(0);
                    }
                    data.extend_from_slice(&buffer[..n]);
                    capture.push(InputFrame::Input {
                        at_us: timestamp(),
                        data,
                    });
                }
                Err(_) => {
                    capture.push(InputFrame::Disconnect { at_us: timestamp() });
                    break;
                }
            }
        } else {
            match rx.recv_timeout(Duration::from_millis(10)) {
                Ok(frame) => capture.push(frame),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    capture.push(InputFrame::Disconnect { at_us: timestamp() });
                    break;
                }
            }
        }
        for label in markers.try_iter().take(32) {
            capture.push(InputFrame::Marker {
                at_us: timestamp(),
                label,
            });
        }
        if dropped.load(Ordering::Relaxed) {
            capture.truncated = true;
        }
    }
    drop(midi_connection);
    for frame in rx.try_iter() {
        capture.push(frame);
    }
    capture.truncated |= dropped.load(Ordering::Relaxed);
    Ok(capture)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observations_keep_bytes_markers_bounds_and_cannot_be_replay_fixtures() {
        let mut capture = InputCapture::new("midi", "Synthetic", "invented", "test").unwrap();
        capture.push(InputFrame::Marker {
            at_us: 5,
            label: "turn left".into(),
        });
        capture.push(InputFrame::Input {
            at_us: 4,
            data: vec![0xb0, 1, 127],
        });
        assert_eq!(capture.frames[0].timestamp(), 4);
        let json = serde_json::to_vec(&capture).unwrap();
        assert!(serde_json::from_slice::<crate::replay::ReportTrace>(&json).is_err());
        for i in 0..MAX_FRAMES {
            capture.push(InputFrame::Input {
                at_us: i as u64 + 6,
                data: vec![0xb0, 1, 1],
            });
        }
        assert!(capture.truncated);
        assert_eq!(capture.frames.len(), MAX_FRAMES);
        let mut large = InputCapture::new("hid", "Synthetic", "invented", "test").unwrap();
        for i in 0..MAX_FRAMES {
            large.push(InputFrame::Input {
                at_us: i as u64,
                data: vec![0; 4096],
            });
        }
        assert!(large.truncated && large.frames.len() < MAX_FRAMES);
        let mut hid = InputCapture::new("hid", "Synthetic", "invented", "test").unwrap();
        hid.push(InputFrame::Input {
            at_us: 0,
            data: vec![0; 4097],
        });
        assert!(hid.truncated && hid.frames.is_empty());
        assert!(capture_input(
            "midi",
            "unused",
            0,
            "Synthetic",
            "test",
            "test",
            mpsc::channel().1
        )
        .is_err());
    }
}
