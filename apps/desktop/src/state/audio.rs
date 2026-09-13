//! Device discovery, switching and recording never block the UI thread.
use super::*;
use mixless_engine::{AudioDevice, RecordingStatus};

#[derive(Default)]
pub struct AudioUi {
    pub open: bool,
    pub devices: Vec<AudioDevice>,
    pub pending: bool,
    pub recording: RecordingStatus,
    devices_rx: Option<Receiver<Result<Vec<AudioDevice>, String>>>,
    action_rx: Option<Receiver<Result<(), String>>>,
}

impl UiState {
    pub fn toggle_audio_menu(&mut self) {
        self.audio.open = !self.audio.open;
        if !self.audio.open {
            return;
        }
        let (tx, rx) = channel();
        self.audio.devices_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(mixless_engine::output_devices().map_err(|e| e.to_string()));
        });
    }

    pub fn select_audio_device(&mut self, monitor: bool, name: Option<String>) {
        if self.audio.pending || self.core.engine.recording_status().active {
            return;
        }
        let core = self.core.clone();
        let (tx, rx) = channel();
        self.audio.pending = true;
        self.audio.action_rx = Some(rx);
        std::thread::spawn(move || {
            let result = (|| {
                let _apply = core
                    .settings_apply
                    .lock()
                    .map_err(|_| "Audio settings unavailable")?;
                let previous = core.engine.audio_config();
                let mut config = previous.clone();
                if monitor {
                    config.cue_device = name;
                } else {
                    config.master_device = name;
                }
                core.engine
                    .configure_audio(config.clone())
                    .map_err(|e| e.to_string())?;
                if let Err(error) = core.settings.update(|s| s.audio = config) {
                    return match core.engine.configure_audio(previous) {
                        Ok(()) => Err(error),
                        Err(rollback) => Err(format!("{error}; audio restore failed: {rollback}")),
                    };
                }
                Ok(())
            })();
            let _ = tx.send(result);
        });
    }

    pub fn toggle_recording(&mut self) {
        if self.audio.pending {
            return;
        }
        let core = self.core.clone();
        let stop = core.engine.recording_status().active;
        let (tx, rx) = channel();
        self.audio.pending = true;
        self.audio.action_rx = Some(rx);
        std::thread::spawn(move || {
            let result = if stop {
                core.engine.stop_recording().map(|_| ())
            } else {
                (|| {
                    let dir = dirs::audio_dir()
                        .or_else(dirs::home_dir)
                        .ok_or("Music folder unavailable")?
                        .join("Mixless Recordings");
                    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_millis();
                    core.engine
                        .start_recording(&dir.join(format!("Mixless-{timestamp}.wav")))
                })()
            };
            let _ = tx.send(result);
        });
    }

    pub(super) fn poll_audio(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = self.audio.devices_rx.take() {
            match rx.try_recv() {
                Ok(Ok(devices)) => {
                    self.audio.devices = devices;
                    changed = true;
                }
                Ok(Err(e)) => {
                    self.error = e.into();
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.audio.devices_rx = Some(rx),
                Err(_) => {
                    self.error = "Device discovery stopped unexpectedly".into();
                    changed = true;
                }
            }
        }
        if let Some(rx) = self.audio.action_rx.take() {
            match rx.try_recv() {
                Ok(result) => {
                    self.audio.pending = false;
                    if let Err(e) = result {
                        self.error = e.into();
                    } else {
                        self.error = "".into();
                    }
                    changed = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self.audio.action_rx = Some(rx),
                Err(_) => {
                    self.audio.pending = false;
                    self.error = "Audio worker stopped unexpectedly".into();
                    changed = true;
                }
            }
        }
        let recording = self.core.engine.recording_status();
        changed |= recording.active != self.audio.recording.active
            || recording.seconds as u64 != self.audio.recording.seconds as u64
            || recording.path != self.audio.recording.path
            || recording.error != self.audio.recording.error;
        if recording.error != self.audio.recording.error {
            if let Some(error) = &recording.error {
                self.error = error.clone().into();
            }
        }
        self.audio.recording = recording;
        changed
    }
}
