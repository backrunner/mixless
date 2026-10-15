//! Controller routing keeps its preceding outputs until the suggestion is cleared.
use super::*;
use crate::settings::ControllerAudioRestore;

pub(in crate::preferences) struct ControllerAudioDraft {
    previous: AudioConfig,
    previous_saved: AudioConfig,
    suggested: AudioConfig,
}

impl ControllerAudioDraft {
    pub(in crate::preferences) fn from_saved(saved: ControllerAudioRestore) -> Self {
        Self {
            previous: saved.previous.clone(),
            previous_saved: saved.previous,
            suggested: saved.suggested,
        }
    }
}

pub(in crate::preferences) struct PendingAudioRestore {
    draft: ControllerAudioDraft,
    audio: AudioConfig,
    preset: Option<String>,
    device: Option<String>,
}

fn same_master(a: &AudioConfig, b: &AudioConfig) -> bool {
    (&a.master_device, &a.master_uid, a.master_channels)
        == (&b.master_device, &b.master_uid, b.master_channels)
}
fn same_cue(a: &AudioConfig, b: &AudioConfig) -> bool {
    (&a.cue_device, &a.cue_uid, a.cue_channels) == (&b.cue_device, &b.cue_uid, b.cue_channels)
}
fn restored_routes(
    current: &AudioConfig,
    previous: &AudioConfig,
    suggested: &AudioConfig,
) -> AudioConfig {
    let mut restored = current.clone();
    // Treat device, stable UID and channels as one output. Preserve manually
    // changed outputs and format fields rather than replacing the whole config.
    if same_master(current, suggested) {
        restored.master_device = previous.master_device.clone();
        restored.master_uid = previous.master_uid.clone();
        restored.master_channels = previous.master_channels;
    }
    if same_cue(current, suggested) {
        restored.cue_device = previous.cue_device.clone();
        restored.cue_uid = previous.cue_uid.clone();
        restored.cue_channels = previous.cue_channels;
    }
    restored
}

impl Preferences {
    pub(super) fn recommended_audio(&self) -> Option<mixless_controllers::AudioRecommendation> {
        self.selected_profile().and_then(|p| p.audio).or_else(|| {
            let device = self
                .controller_devices
                .iter()
                .find(|d| Some(&d.id) == self.controller_device.as_ref())?;
            mixless_controllers::ni_audio_recommendation(&device.name)
        })
    }

    pub(in crate::preferences) fn apply_recommended_audio(&mut self) {
        let Some(audio) = self.recommended_audio() else {
            return;
        };
        if self.job.is_some() {
            return;
        }
        // Repeated recommendations, including an already saved one, keep the
        // first baseline and do not temporarily switch the live output.
        let (previous, previous_saved) = if let Some(draft) = self.controller_audio_draft.take() {
            self.audio = restored_routes(&self.audio, &draft.previous, &draft.suggested);
            (draft.previous, draft.previous_saved)
        } else {
            (self.audio.clone(), self.core.engine.audio_config())
        };
        self.audio.master_uid = None;
        self.audio.cue_uid = None;
        self.audio.master_device = Some(audio.device_name.clone());
        self.audio.cue_device = Some(audio.device_name);
        self.audio.master_channels = audio.master;
        self.audio.cue_channels = audio.cue;
        self.controller_audio_draft = Some(ControllerAudioDraft {
            previous,
            previous_saved,
            suggested: self.audio.clone(),
        });
        self.tab = Tab::Audio;
        self.result(Ok(()), "Review the suggested channels, then choose Apply");
    }

    pub(in crate::preferences) fn controller_audio_backup(
        &self,
        config: &AudioConfig,
    ) -> Option<ControllerAudioRestore> {
        let draft = self.controller_audio_draft.as_ref()?;
        (same_master(config, &draft.suggested) || same_cue(config, &draft.suggested)).then(|| {
            ControllerAudioRestore {
                previous: draft.previous_saved.clone(),
                suggested: draft.suggested.clone(),
            }
        })
    }

    pub(in crate::preferences) fn controller_audio_saved(&mut self) {
        // Apply keeps the backup on disk and in this view. Reopening Preferences
        // reconstructs it from Settings, so None can still restore the outputs.
        if self.core.settings.get().controller_audio_restore.is_none() {
            self.controller_audio_draft = None;
        }
    }

    pub(in crate::preferences) fn restore_controller_audio_draft(&mut self) -> bool {
        if self.job.is_some() {
            return false;
        }
        let Some(draft) = self.controller_audio_draft.take() else {
            return true;
        };
        let current = self.audio.clone();
        self.audio = restored_routes(&current, &draft.previous, &draft.suggested);
        let settings = self.core.settings.get();
        if let Some(saved) = settings.controller_audio_restore {
            // Restore only the saved routes. Unapplied format/output edits stay
            // in the UI draft and must not be silently committed by choosing None.
            let config = restored_routes(&settings.audio, &saved.previous, &saved.suggested);
            self.controller_audio_restore = Some(PendingAudioRestore {
                draft,
                audio: current,
                preset: self.selected_preset.clone(),
                device: self.controller_device.clone(),
            });
            let core = self.core.clone();
            self.status = "Restoring previous audio routing...".into();
            self.error = false;
            self.start_job(move || {
                JobResult::ControllerAudioRestored(audio::save_audio_settings(&core, config, None))
            });
        } else {
            self.result(Ok(()), "Suggested audio routing cleared");
        }
        true
    }

    pub(in crate::preferences) fn finish_controller_audio_restore(
        &mut self,
        result: Result<(), String>,
    ) {
        let Some(pending) = self.controller_audio_restore.take() else {
            return;
        };
        if result.is_err() {
            self.audio = pending.audio;
            self.selected_preset = pending.preset;
            self.controller_device = pending.device.clone();
            self.binding.device_id = pending.device;
            self.controller_audio_draft = Some(pending.draft);
        }
        self.result(result, "Previous audio routing restored");
    }
}
