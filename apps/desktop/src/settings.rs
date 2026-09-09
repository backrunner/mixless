use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use mixless_engine::AudioConfig;
use mixless_midi::MidiConfig;
use mixless_protocol::{Command, DeckId, XfCurve};
use serde::{Deserialize, Serialize};

use crate::state::WaveLayout;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub audio: AudioConfig,
    pub midi: MidiConfig,
    pub wave_layout: WaveLayout,
    pub show_fx: bool,
    pub quantize: bool,
    pub filter_resonance: bool,
    pub xf_curve: XfCurve,
    pub xf_reverse: bool,
    pub cue_gain: f32,
    pub keylock: bool,
    pub vinyl: bool,
    pub slip: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            audio: AudioConfig::default(),
            midi: MidiConfig::default(),
            wave_layout: WaveLayout::Top,
            show_fx: true,
            quantize: true,
            filter_resonance: true,
            xf_curve: XfCurve::EqualPower,
            xf_reverse: false,
            cue_gain: 0.8,
            keylock: true,
            vinyl: false,
            slip: false,
        }
    }
}

impl Settings {
    fn validate(&self) -> Result<(), String> {
        if !self.cue_gain.is_finite() || !(0.0..=1.0).contains(&self.cue_gain) {
            return Err("Headphone volume must be between 0 and 100%.".into());
        }
        if self
            .audio
            .sample_rate
            .is_some_and(|rate| !(8_000..=96_000).contains(&rate))
        {
            return Err("Audio sample rate must be between 8000 and 96000 Hz.".into());
        }
        if self
            .audio
            .buffer_frames
            .is_some_and(|frames| frames == 0 || frames > 8192)
        {
            return Err("Invalid audio buffer size.".into());
        }
        Ok(())
    }

    pub fn apply_playback(&self, engine: &mixless_engine::Engine) {
        for command in [
            Command::SetQuantize { on: self.quantize },
            Command::SetXfCurve {
                curve: self.xf_curve,
            },
            Command::SetXfReverse {
                on: self.xf_reverse,
            },
            Command::SetCueGain {
                value: self.cue_gain,
            },
        ] {
            let _ = engine.dispatch(command);
        }
        for deck in [DeckId::A, DeckId::B] {
            let _ = engine.dispatch(Command::SetFilterResonanceEnabled { deck, on: self.filter_resonance });
            let _ = engine.dispatch(Command::SetKeyLock {
                deck,
                on: self.keylock,
            });
            let _ = engine.dispatch(Command::SetVinylMode {
                deck,
                vinyl: self.vinyl,
                slip: self.slip,
            });
        }
    }
}

pub struct SettingsStore {
    pub path: PathBuf,
    value: Mutex<Settings>,
    revision: AtomicU64,
    pub load_error: Option<String>,
}

impl SettingsStore {
    pub fn load(path: PathBuf) -> Self {
        let result = if path.exists() {
            std::fs::read(&path)
                .map_err(|e| e.to_string())
                .and_then(|data| {
                    let settings: Settings =
                        serde_json::from_slice(&data).map_err(|e| e.to_string())?;
                    settings.validate()?;
                    Ok(settings)
                })
        } else {
            Ok(Settings::default())
        };
        let (settings, load_error) = match result {
            Ok(value) => (value, None),
            Err(error) => (
                Settings::default(),
                Some(format!("Could not load preferences: {error}")),
            ),
        };
        Self {
            path,
            value: Mutex::new(settings),
            revision: AtomicU64::new(1),
            load_error,
        }
    }

    pub fn get(&self) -> Settings {
        self.value.lock().expect("preferences").clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    pub fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<(), String> {
        let mut value = self.value.lock().expect("preferences");
        let mut next = value.clone();
        change(&mut next);
        next.validate()?;
        let data = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
        let temp = self.path.with_extension("json.tmp");
        std::fs::write(&temp, data).map_err(|e| format!("Could not save preferences: {e}"))?;
        std::fs::rename(&temp, &self.path)
            .map_err(|e| format!("Could not save preferences: {e}"))?;
        *value = next;
        self.revision.fetch_add(1, Ordering::Release);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_settings_preserve_defaults_and_reject_invalid_audio() {
        let settings: Settings =
            serde_json::from_str(r#"{"show_fx":false,"audio":{"master_device":"USB"}}"#).unwrap();
        assert!(!settings.show_fx);
        assert!(settings.quantize);
        assert!(settings.midi.enabled);
        assert_eq!(settings.audio.master_device.as_deref(), Some("USB"));
        assert!(
            Settings {
                cue_gain: f32::NAN,
                ..Settings::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn preferences_round_trip_and_failed_save_preserves_memory() {
        let dir = std::env::temp_dir().join(format!("mixless-settings-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        let store = SettingsStore::load(path.clone());
        store
            .update(|s| {
                s.show_fx = false;
                s.midi.enabled = false;
            })
            .unwrap();
        assert!(!SettingsStore::load(path).get().midi.enabled);
        let invalid = SettingsStore::load(dir.join("missing/settings.json"));
        assert!(invalid.update(|s| s.show_fx = false).is_err());
        assert!(invalid.get().show_fx);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
