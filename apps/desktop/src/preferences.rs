use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gpui::{
    App, Bounds, Context, FocusHandle, IntoElement, Render, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use mixless_controllers::{
    MidiBinding, MidiConfig, MidiControlMode, MidiGroup, MidiMessage, MidiPort, MidiSourceKind,
    MidiTarget,
};
use mixless_engine::{AudioConfig, AudioDevice};
use mixless_protocol::{Command, DeckId, LiveMoves, XfCurve};

use crate::{
    settings::Settings,
    state::{AppCore, WaveLayout},
    storage::{self, ClearKind, StorageReport},
    theme,
};

mod audio;
mod controllers;
mod general;
mod midi;
#[cfg(debug_assertions)]
pub mod preview;
mod select;
#[cfg(test)]
mod tests;
mod widgets;
use controllers::PresetDialog;

use widgets::{setting_detail, setting_group, setting_stack};

const TITLEBAR_HEIGHT: f32 = 50.;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    General,
    Audio,
    Midi,
    Storage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Selector {
    Master,
    Cue,
    MasterChannels,
    CueChannels,
    Rate,
    Depth,
    Buffer,
    Source,
    MidiChannel,
    MidiNumber,
    MidiMode,
    MidiPreset,
    ControllerDevice,
    PresetActions,
}

enum JobResult {
    Devices(
        Result<Vec<AudioDevice>, String>,
        Result<Vec<MidiPort>, String>,
    ),
    Audio(Result<(), String>),
    Midi(Result<(), String>),
    File(Result<String, String>),
    PresetImport(Result<mixless_controllers::ImportReport, String>),
    Dir(Option<PathBuf>),
    Storage(Result<StorageReport, String>),
    StorageClear(Result<(u64, StorageReport), String>),
}

pub struct Preferences {
    core: Arc<AppCore>,
    focus: FocusHandle,
    tab: Tab,
    selector: Option<Selector>,
    select_ui: select::SelectState,
    window_observers: Vec<gpui::Subscription>,
    audio: AudioConfig,
    midi: MidiConfig,
    devices: Vec<AudioDevice>,
    ports: Vec<MidiPort>,
    bindings: Vec<MidiBinding>,
    binding: MidiBinding,
    editing: Option<usize>,
    midi_group: Option<MidiGroup>,
    midi_inputs_open: bool,
    midi_scroll: gpui::UniformListScrollHandle,
    learning: bool,
    quick_learning: bool,
    midi_feedback: Option<(String, bool)>,
    last: Option<MidiMessage>,
    job: Option<mpsc::Receiver<JobResult>>,
    status: String,
    error: bool,
    storage: Option<StorageReport>,
    confirm_clear: Option<ClearKind>,
    confirm_reset: bool,
    controller_advanced: bool,
    controller_notes: bool,
    controller_monitor: bool,
    audio_details_open: bool,
    controller_device: Option<String>,
    controller_devices: Vec<mixless_controllers::ControllerDevice>,
    selected_preset: Option<String>,
    preset_dialog: Option<PresetDialog>,
    import_preview: Option<mixless_controllers::ImportReport>,
}

impl Preferences {
    fn new(core: Arc<AppCore>, cx: &mut Context<Self>) -> Self {
        let settings = core.settings.get();
        let status = core.settings.load_error.clone().unwrap_or_default();
        let mut state = Self {
            core,
            focus: cx.focus_handle(),
            tab: Tab::General,
            selector: None,
            select_ui: Default::default(),
            window_observers: Vec::new(),
            audio: settings.audio,
            midi: settings.midi,
            devices: Vec::new(),
            ports: Vec::new(),
            bindings: Vec::new(),
            binding: MidiBinding {
                device_id: None,
                kind: MidiSourceKind::Cc,
                channel: 0,
                number: 0,
                target: MidiTarget::Xfader,
                mode: MidiControlMode::Auto,
            },
            editing: None,
            midi_group: None,
            midi_inputs_open: false,
            midi_scroll: gpui::UniformListScrollHandle::new(),
            learning: false,
            quick_learning: false,
            midi_feedback: None,
            last: None,
            job: None,
            error: !status.is_empty(),
            status,
            storage: None,
            confirm_clear: None,
            confirm_reset: false,
            controller_advanced: false,
            controller_notes: false,
            controller_monitor: false,
            audio_details_open: false,
            controller_device: None,
            controller_devices: Vec::new(),
            selected_preset: None,
            preset_dialog: None,
            import_preview: None,
        };
        state.refresh();
        cx.on_release(|state, _| state.cancel_learn()).detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if this
                    .update(cx, |state, cx| {
                        state.poll();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        state
    }

    fn start_job(&mut self, job: impl FnOnce() -> JobResult + Send + 'static) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(job());
        });
    }

    fn refresh(&mut self) {
        self.start_job(|| {
            JobResult::Devices(
                mixless_engine::output_devices().map_err(|e| e.to_string()),
                mixless_controllers::input_ports().map_err(|e| e.to_string()),
            )
        });
    }

    fn result(&mut self, result: Result<(), String>, success: &str) {
        match result {
            Ok(()) => {
                self.status = success.into();
                self.error = false;
            }
            Err(error) => {
                self.status = error;
                self.error = true;
            }
        }
    }

    fn poll(&mut self) {
        if let Some(hub) = self.core.midi.lock().expect("controllers").as_ref() {
            self.controller_devices = hub.devices();
            self.ports = self
                .controller_devices
                .iter()
                .filter(|d| !d.hid && d.state != mixless_controllers::ConnectionState::Offline)
                .map(|d| MidiPort {
                    id: d.id.clone(),
                    name: d.name.clone(),
                })
                .collect();
        }
        if let Some(rx) = self.job.take() {
            match rx.try_recv() {
                Ok(JobResult::Devices(audio, midi)) => {
                    match audio {
                        Ok(devices) => self.devices = devices,
                        Err(e) => self.result(Err(e), ""),
                    }
                    match midi {
                        Ok(ports) => self.ports = ports,
                        Err(e) => self.result(Err(e), ""),
                    }
                }
                Ok(JobResult::Audio(result)) => self.result(result, "Audio settings saved"),
                Ok(JobResult::Midi(result)) => self.result(result, "MIDI inputs saved"),
                Ok(JobResult::PresetImport(result)) => match result {
                    Ok(report) => {
                        self.result(Ok(()), "");
                        self.import_preview = Some(report);
                    }
                    Err(error) => self.result(Err(error), ""),
                },
                Ok(JobResult::File(result)) => match result {
                    Ok(message) => self.result(
                        Ok(()),
                        if message.is_empty() {
                            "MIDI mapping file updated"
                        } else {
                            &message
                        },
                    ),
                    Err(error) => self.result(Err(error), ""),
                },
                Ok(JobResult::Storage(result)) => match result {
                    Ok(report) => self.storage = Some(report),
                    Err(error) => self.result(Err(error), ""),
                },
                Ok(JobResult::StorageClear(result)) => match result {
                    Ok((freed, report)) => {
                        self.storage = Some(report);
                        self.result(Ok(()), &format!("Freed {}", storage::format_bytes(freed)));
                    }
                    Err(error) => {
                        self.result(Err(error), "");
                        self.refresh_storage();
                    }
                },
                Ok(JobResult::Dir(dir)) => {
                    if let Some(dir) = dir {
                        let result = crate::state::ensure_writable_dir(&dir).and_then(|()| {
                            self.core.settings.update(|s| s.download_dir = Some(dir))
                        });
                        self.result(result, "Download folder saved");
                    }
                }
                Err(mpsc::TryRecvError::Empty) => self.job = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.result(Err("Settings operation stopped unexpectedly".into()), "")
                }
            }
        }
        let midi = self
            .core
            .midi
            .lock()
            .expect("MIDI")
            .as_ref()
            .map(|hub| (hub.bindings(), hub.last_message(), hub.learned()));
        if let Some((bindings, last, learned)) = midi {
            if self.bindings != bindings {
                self.bindings = bindings;
                self.select_midi_target(self.binding.target.clone());
            }
            self.last = last;
            if self.learning {
                if let Some(message) = learned {
                    if self.quick_learning {
                        self.finish_quick_learn(message);
                        return;
                    }
                    self.binding.device_id = Some(message.device_id);
                    self.binding.kind = message.kind;
                    self.binding.channel = message.channel;
                    self.binding.number = message.number;
                }
            }
        }
    }

    fn save_general(&mut self, change: impl FnOnce(&mut Settings)) {
        let before = self.core.settings.get();
        let result = self.core.settings.update(change);
        if result.is_ok() {
            let after = self.core.settings.get();
            if before.deep_analysis != after.deep_analysis {
                // Retry basic-only rows when enabled; disabling stops queued
                // model work at the next chunk without interrupting playback.
                let core = self.core.clone();
                std::thread::spawn(move || {
                    if let Ok(tracks) = core.library.list_tracks() {
                        for track in &tracks {
                            core.analysis.forget(track.id);
                        }
                        crate::analysis::schedule(&core, &tracks);
                        crate::automix::refresh_previews(&core);
                    }
                });
            }
            let engine = &self.core.engine;
            if before.fx_auto_fade != after.fx_auto_fade {
                let _ = engine.dispatch(Command::SetFxAutoFade {
                    on: after.fx_auto_fade,
                });
            }
            if before.quantize != after.quantize {
                let _ = engine.dispatch(Command::SetQuantize { on: after.quantize });
            }
            if before.xf_curve != after.xf_curve {
                let _ = engine.dispatch(Command::SetXfCurve {
                    curve: after.xf_curve,
                });
            }
            if before.xf_reverse != after.xf_reverse {
                let _ = engine.dispatch(Command::SetXfReverse {
                    on: after.xf_reverse,
                });
            }
            if before.cue_gain != after.cue_gain {
                let _ = engine.dispatch(Command::SetCueGain {
                    value: after.cue_gain,
                });
            }
            for deck in [DeckId::A, DeckId::B] {
                if before.filter_resonance != after.filter_resonance {
                    let _ = engine.dispatch(Command::SetFilterResonanceEnabled {
                        deck,
                        on: after.filter_resonance,
                    });
                }
                if before.keylock != after.keylock {
                    let _ = engine.dispatch(Command::SetKeyLock {
                        deck,
                        on: after.keylock,
                    });
                }
                if before.vinyl != after.vinyl || before.slip != after.slip {
                    let _ = engine.dispatch(Command::SetVinylMode {
                        deck,
                        vinyl: after.vinyl,
                        slip: after.slip,
                    });
                }
            }
        }
        self.result(result, "Preferences saved");
    }

    fn apply_audio(&mut self) {
        let core = self.core.clone();
        let config = self.audio.clone();
        self.status = "Applying audio settings...".into();
        self.error = false;
        self.start_job(move || {
            JobResult::Audio((|| {
                let _apply = core.settings_apply.lock().expect("apply settings");
                let previous = core.engine.audio_config();
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
            })())
        });
    }

    fn apply_midi(&mut self) {
        self.cancel_learn();
        let core = self.core.clone();
        let config = self.midi.clone();
        self.status = "Connecting MIDI inputs...".into();
        self.error = false;
        self.start_job(move || {
            JobResult::Midi((|| {
                let _apply = core.settings_apply.lock().expect("apply settings");
                let previous = core.settings.get().midi;
                let midi = core.midi.lock().expect("MIDI").clone();
                let hub = if let Some(hub) = midi {
                    hub.configure(config.clone()).map_err(|e| e.to_string())?;
                    hub
                } else {
                    let path = core.settings.path.with_file_name("midi.json");
                    let hub = Arc::new(
                        mixless_controllers::MidiHub::start_with_config(path, config.clone())
                            .map_err(|e| e.to_string())?,
                    );
                    *core.midi.lock().expect("MIDI") = Some(hub.clone());
                    hub
                };
                if let Err(error) = core.settings.update(|s| s.midi = config) {
                    if let Err(rollback) = hub.configure(previous) {
                        return Err(format!("{error}; MIDI restore failed: {rollback}"));
                    }
                    return Err(error);
                }
                core.attach_controller_runtime();
                *core.midi_error.lock().expect("MIDI error") = None;
                Ok(())
            })())
        });
    }

    fn cancel_learn(&mut self) {
        self.learning = false;
        self.quick_learning = false;
        if let Some(hub) = self.core.midi.lock().expect("MIDI").as_ref() {
            hub.set_learn(false);
        }
    }

    fn toggle_learn(&mut self) {
        if self.learning {
            self.cancel_learn();
            return;
        }
        self.midi_feedback = None;
        if self.controller_device.is_none() {
            self.result(Err("Select a connected device before learning".into()), "");
            return;
        }
        if let Some(hub) = self.core.midi.lock().expect("MIDI").as_ref() {
            match hub.learn_target_for(self.binding.target.clone(), self.controller_device.clone())
            {
                Ok(()) => self.learning = true,
                Err(e) => {
                    self.learning = false;
                    self.midi_feedback = Some((e.to_string(), true));
                }
            }
        }
    }

    fn save_binding(&mut self) {
        if self.controller_device.is_none() && self.editing.is_none() {
            self.result(
                Err(
                    "Select a device before assigning; use Advanced mapping for offline presets"
                        .into(),
                ),
                "",
            );
            return;
        }
        let result = {
            let midi = self.core.midi.lock().expect("MIDI");
            midi.as_ref()
                .ok_or_else(|| "MIDI is unavailable".to_string())
                .and_then(|hub| {
                    if self.learning {
                        let message = hub
                            .learned()
                            .ok_or_else(|| "Waiting for a MIDI signal".to_string())?;
                        self.binding.device_id = Some(message.device_id);
                        self.binding.kind = message.kind;
                        self.binding.channel = message.channel;
                        self.binding.number = message.number;
                    }
                    if let Some(index) = self.editing {
                        hub.replace(index, self.binding.clone())
                            .map_err(|e| e.to_string())
                    } else {
                        hub.upsert(self.binding.clone()).map_err(|e| e.to_string())
                    }
                })
        };
        let saved = result.is_ok().then(|| self.binding.clone());
        if saved.is_some() {
            self.cancel_learn();
        }
        self.result(result, "MIDI mapping saved");
        self.poll();
        if let Some(saved) = saved {
            self.editing = self.bindings.iter().position(|binding| *binding == saved);
            self.binding = saved;
        }
    }

    fn file_dialog(&mut self, export: bool, cx: &mut Context<Self>) {
        if self.job.is_some() {
            return;
        }
        self.cancel_learn();
        let core = self.core.clone();
        let selected = self.selected_preset.clone();
        if export && selected.is_none() {
            self.result(Err("Choose a preset to export".into()), "");
            return;
        }
        self.result(Ok(()), "");
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        cx.spawn(async move |_, _| {
            let dialog =
                rfd::AsyncFileDialog::new().add_filter("mixless controller preset", &["json"]);
            let file = if export {
                dialog
                    .set_file_name("controller.mixless.json")
                    .save_file()
                    .await
            } else {
                dialog.pick_file().await
            };
            let result = if let Some(file) = file {
                let hub = core.midi.lock().expect("controllers").clone();
                if export {
                    JobResult::File(
                        hub.ok_or("Controllers unavailable".to_string())
                            .and_then(|h| {
                                h.export_preset(selected.as_deref().unwrap(), file.path())
                                    .map_err(|e| e.to_string())
                            })
                            .map(|_| "Preset exported".into()),
                    )
                } else {
                    JobResult::PresetImport(
                        hub.ok_or("Controllers unavailable".to_string())
                            .and_then(|h| h.preview_import(file.path()).map_err(|e| e.to_string())),
                    )
                }
            } else {
                JobResult::File(Ok("Cancelled".into()))
            };
            let _ = tx.send(result);
        })
        .detach();
    }

    fn pick_download_dir(&mut self, cx: &mut Context<Self>) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        cx.spawn(async move |_, _| {
            let dir = rfd::AsyncFileDialog::new()
                .set_title("Save downloaded audio to")
                .pick_folder()
                .await
                .map(|folder| folder.path().to_path_buf());
            let _ = tx.send(JobResult::Dir(dir));
        })
        .detach();
    }

    fn refresh_storage(&mut self) {
        if self.job.is_some() {
            return;
        }
        let core = self.core.clone();
        self.start_job(move || JobResult::Storage(storage::report(&core)));
    }

    fn clear_storage(&mut self, kind: ClearKind) {
        let core = self.core.clone();
        self.status = "Clearing...".into();
        self.error = false;
        self.start_job(move || {
            JobResult::StorageClear(
                storage::clear(&core, kind)
                    .and_then(|freed| storage::report(&core).map(|report| (freed, report))),
            )
        });
    }

    /// A cache row: name and what it holds on the left, size plus an optional
    /// two-step Clear on the right. The first click arms; the second clears.
    fn storage_row(
        &self,
        id: impl Into<SharedString>,
        label: &str,
        detail: &str,
        bytes: u64,
        kind: Option<ClearKind>,
        cx: &Context<Self>,
    ) -> gpui::Div {
        let busy = self.job.is_some();
        let armed = kind.is_some() && self.confirm_clear == kind;
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_4()
            .min_h(px(56.))
            .px_4()
            .py_3()
            .border_b_1()
            .border_color(theme::LINE_SOFT)
            .text_size(px(12.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().child(label.to_string()))
                    .when(!detail.is_empty(), |el| {
                        el.child(
                            div()
                                .text_size(px(11.))
                                .text_color(theme::MUTED)
                                .child(detail.to_string()),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .flex_none()
                    .child(
                        div()
                            .text_color(theme::MUTED)
                            .child(storage::format_bytes(bytes)),
                    )
                    .when_some(kind, |el, kind| {
                        el.child(
                            self.button(
                                format!("clear-{}", id.into()),
                                if armed { "Confirm clear" } else { "Clear" },
                                !busy && (bytes > 0 || armed),
                                move |s, _, _| {
                                    if s.confirm_clear == Some(kind) {
                                        s.confirm_clear = None;
                                        s.clear_storage(kind);
                                    } else {
                                        s.confirm_clear = Some(kind);
                                    }
                                },
                                cx,
                            )
                            .when(armed, |el| el.text_color(theme::DANGER)),
                        )
                    }),
            )
    }

    fn storage_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let busy = self.job.is_some();
        let mut page = setting_stack().w_full().max_w(px(760.)).mx_auto()
            .child(div().text_size(px(11.)).line_height(px(16.)).text_color(theme::MUTED)
                .child("Clear cached data to reclaim space. Source audio, playlists and saved cues are kept."));
        let Some(report) = &self.storage else {
            return page
                .child(info(
                    "Usage",
                    if busy {
                        "Computing...".into()
                    } else {
                        "Not computed".into()
                    },
                ))
                .child(div().flex().justify_end().py_3().child(self.button(
                    "storage-scan",
                    "Compute usage",
                    !busy,
                    |s, _, _| s.refresh_storage(),
                    cx,
                )))
                .into_any_element();
        };
        page = page
            .child(setting_group(
                "Cached data",
                vec![
                    self.storage_row(
                        "stems",
                        "Stem cache",
                        "Separated vocals, drums and instruments",
                        report.stems,
                        Some(ClearKind::Stems),
                        cx,
                    ),
                    self.storage_row(
                        "analysis",
                        "Track analysis",
                        "Beat grids, keys and song structure",
                        report.analysis,
                        Some(ClearKind::Analysis),
                        cx,
                    ),
                    self.storage_row(
                        "waveforms",
                        "Waveforms",
                        "Rebuilt from audio when needed",
                        report.waveforms,
                        Some(ClearKind::Waveforms),
                        cx,
                    ),
                    self.storage_row(
                        "artwork",
                        "Artwork",
                        "Rebuilt on next launch",
                        report.artwork,
                        Some(ClearKind::Artwork),
                        cx,
                    ),
                    self.storage_row(
                        "models",
                        "Analysis models",
                        "Downloaded copies only; models included in the app are kept",
                        report.models,
                        Some(ClearKind::Models),
                        cx,
                    ),
                ],
            ))
            .child(setting_group(
                "Library",
                vec![
                    self.storage_row(
                        "database",
                        "Library database",
                        "Tracks, cues, playlists and analysis",
                        report.database,
                        None,
                        cx,
                    ),
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_4()
                        .min_h(px(56.))
                        .px_4()
                        .py_3()
                        .border_b_1()
                        .border_color(theme::LINE_SOFT)
                        .text_size(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().child("Downloaded audio"))
                                .child(
                                    div().text_size(px(11.)).text_color(theme::MUTED).child(
                                        crate::state::display_dir(&self.core.download_dir()),
                                    ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .flex_none()
                                .child(
                                    div()
                                        .text_color(theme::MUTED)
                                        .child(storage::format_bytes(report.acquired)),
                                )
                                .child(
                                    self.button(
                                        "storage-acquired",
                                        "Show in Finder",
                                        true,
                                        |s, _, cx| {
                                            let dir = s.core.download_dir();
                                            std::fs::create_dir_all(&dir).ok();
                                            cx.reveal_path(&dir);
                                        },
                                        cx,
                                    )
                                    .into_any_element(),
                                ),
                        ),
                ],
            ));
        if !report.playlists.is_empty() {
            page = page
                .child(setting_group(
                    "Per playlist",
                    report
                        .playlists
                        .iter()
                        .map(|playlist| {
                            self.storage_row(
                                format!("playlist-{}", playlist.id),
                                &playlist.name,
                                &format!("{} tracks", playlist.tracks),
                                playlist.bytes,
                                Some(ClearKind::Playlist(playlist.id)),
                                cx,
                            )
                        })
                        .collect(),
                ))
                .child(
                    div()
                        .text_size(px(11.))
                        .line_height(px(16.))
                        .text_color(theme::MUTED)
                        .child("Tracks shared with other playlists use the same caches."),
                );
        }
        page.child(div().flex().justify_end().py_3().child(self.button(
            "storage-refresh",
            "Refresh usage",
            !busy,
            |s, _, _| s.refresh_storage(),
            cx,
        )))
        .into_any_element()
    }

    fn button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        enabled: bool,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.styled_button(id, label, enabled, false, action, cx)
    }

    fn primary_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        enabled: bool,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        self.styled_button(id, label, enabled, true, action, cx)
    }

    fn styled_button(
        &self,
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        enabled: bool,
        primary: bool,
        action: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        let id = id.into();
        div()
            .debug_selector(|| id.to_string())
            .id(id)
            .h(px(widgets::CONTROL_HEIGHT))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .rounded(px(widgets::CONTROL_RADIUS))
            .border_1()
            .border_color(theme::with_alpha(
                theme::TEXT,
                if enabled { 0.12 } else { 0.05 },
            ))
            .bg(if primary && enabled {
                theme::TEXT
            } else {
                theme::PANEL_RAISED
            })
            .text_size(px(12.))
            .text_color(if !enabled {
                theme::MUTED
            } else if primary {
                theme::BG
            } else {
                theme::TEXT
            })
            .when(primary, |el| el.font_weight(gpui::FontWeight::MEDIUM))
            .when(enabled, |el| {
                el.cursor_pointer().hover(move |el| {
                    el.bg(if primary {
                        theme::with_alpha(theme::TEXT, 0.9)
                    } else {
                        theme::LINE.into()
                    })
                })
            })
            .child(div().min_w_0().child(label.into()))
            .on_click(cx.listener(move |state, _, window, cx| {
                if enabled {
                    action(state, window, cx);
                    cx.notify();
                }
            }))
    }

    fn toggle(
        &self,
        id: &'static str,
        on: bool,
        action: impl Fn(&mut Self) + 'static,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .w(px(40.))
            .h(px(22.))
            .px(px(2.))
            .flex()
            .items_center()
            .flex_none()
            .rounded(px(999.))
            .bg(if on { theme::ACCENT } else { theme::LINE })
            .when(on, |el| el.justify_end())
            .cursor_pointer()
            .child(
                div()
                    .size(px(18.))
                    .rounded(px(999.))
                    .bg(theme::TEXT)
                    .shadow_sm(),
            )
            .on_click(cx.listener(move |s, _, _, cx| {
                action(s);
                cx.notify();
            }))
            .into_any_element()
    }

    fn port_name(&self, id: &str) -> String {
        self.ports
            .iter()
            .find(|p| p.id == id)
            .map_or_else(|| id.into(), |p| p.name.clone())
    }

    fn selector_options(&self, selector: Selector) -> Vec<(String, String)> {
        match selector {
            Selector::Master | Selector::Cue => {
                let mut options = vec![(
                    String::new(),
                    if selector == Selector::Master {
                        "System default"
                    } else {
                        "Disabled"
                    }
                    .into(),
                )];
                options.extend(
                    self.devices
                        .iter()
                        .map(|d| (d.name.clone(), d.name.clone())),
                );
                options
            }
            Selector::Rate => {
                let mut options = vec![(String::new(), "Device default".into())];
                let master = self
                    .audio
                    .master_device
                    .clone()
                    .unwrap_or_else(|| self.core.engine.snapshot().device_name);
                let rates = self
                    .devices
                    .iter()
                    .find(|d| d.name == master)
                    .map(|d| d.sample_rates.as_slice())
                    .unwrap_or(&AudioConfig::SAMPLE_RATES);
                let device = self.devices.iter().find(|d| d.name == master);
                options.extend(
                    rates
                        .iter()
                        .filter(|rate| {
                            self.audio.bit_depth.is_none_or(|bits| {
                                device.is_some_and(|d| {
                                    d.output_formats
                                        .iter()
                                        .any(|f| f.sample_rate == **rate && f.bit_depth == bits)
                                })
                            })
                        })
                        .map(|rate| (rate.to_string(), format!("{rate} Hz"))),
                );
                options
            }
            Selector::Depth => {
                let mut options = vec![(String::new(), "Device default".into())];
                let master = self
                    .audio
                    .master_device
                    .clone()
                    .unwrap_or_else(|| self.core.engine.snapshot().device_name);
                if let Some(device) = self.devices.iter().find(|d| d.name == master) {
                    let rate = self.audio.sample_rate.unwrap_or(device.current_sample_rate);
                    let mut bits = device
                        .output_formats
                        .iter()
                        .filter(|f| f.sample_rate == rate)
                        .map(|f| f.bit_depth)
                        .collect::<Vec<_>>();
                    bits.sort_unstable();
                    bits.dedup();
                    options.extend(
                        bits.into_iter()
                            .map(|n| (n.to_string(), format!("{n}-bit"))),
                    );
                }
                options
            }
            Selector::Buffer => {
                let mut options =
                    vec![(String::new(), AudioConfig::automatic_buffer_label().into())];
                options.extend(
                    [64, 128, 256, 512, 1024, 2048].map(|n| (n.to_string(), format!("{n} frames"))),
                );
                options
            }
            Selector::MasterChannels | Selector::CueChannels => {
                let name = if selector == Selector::MasterChannels {
                    self.audio
                        .master_device
                        .clone()
                        .unwrap_or_else(|| self.core.engine.snapshot().device_name)
                } else {
                    self.audio.cue_device.clone().unwrap_or_default()
                };
                let count = self
                    .devices
                    .iter()
                    .find(|d| d.name == name)
                    .map_or(0, |d| d.output_channels);
                (0..count.saturating_sub(1))
                    .step_by(2)
                    .map(|n| (n.to_string(), format!("{} / {}", n + 1, n + 2)))
                    .collect()
            }
            Selector::MidiChannel => (1..=16)
                .map(|n| (n.to_string(), format!("Channel {n}")))
                .collect(),
            Selector::MidiNumber => (0..=127).map(|n| (n.to_string(), n.to_string())).collect(),
            Selector::MidiMode => MidiControlMode::ALL
                .into_iter()
                .enumerate()
                .map(|(i, mode)| (i.to_string(), mode.label().into()))
                .collect(),
            Selector::MidiPreset => self
                .core
                .midi
                .lock()
                .expect("controllers")
                .as_ref()
                .map(|hub| hub.presets().into_iter().map(|p| (p.id, p.name)).collect())
                .unwrap_or_default(),
            Selector::ControllerDevice => {
                let mut options = vec![(String::new(), "No device".into())];
                let mut devices: Vec<_> = self
                    .controller_devices
                    .iter()
                    .filter(|d| {
                        !(d.hid && d.usage_page == Some(1) && matches!(d.usage, Some(2 | 6)))
                    })
                    .collect();
                // Put musical MIDI ports before the computer's many HID interfaces.
                devices.sort_by(|a, b| (a.hid, &a.name, &a.id).cmp(&(b.hid, &b.name, &b.id)));
                options.extend(devices.iter().map(|d| {
                    let duplicates =
                        devices.iter().filter(|other| other.name == d.name).count() > 1;
                    let label = if duplicates {
                        // Instance identity is needed only when names cannot distinguish devices.
                        format!(
                            "{} ({})",
                            d.name,
                            d.interface
                                .filter(|n| devices
                                    .iter()
                                    .filter(
                                        |other| other.name == d.name && other.interface == Some(*n)
                                    )
                                    .count()
                                    == 1)
                                .map_or_else(|| d.id.clone(), |n| format!("Interface {n}"))
                        )
                    } else {
                        d.name.clone()
                    };
                    (d.id.clone(), label)
                }));
                options
            }
            Selector::PresetActions => self.preset_actions(),
            Selector::Source => {
                let mut options = vec![(String::new(), "Any enabled input".into())];
                options.extend(self.ports.iter().map(|p| (p.id.clone(), p.name.clone())));
                options
            }
        }
    }

    fn choose(&mut self, selector: Selector, value: &str, cx: &mut Context<Self>) {
        let optional = (!value.is_empty()).then(|| value.to_string());
        match selector {
            Selector::Master => {
                self.audio.master_uid = self
                    .devices
                    .iter()
                    .find(|d| optional.as_ref() == Some(&d.name))
                    .and_then(|d| d.uid.clone());
                self.audio.master_device = optional;
                self.audio.master_channels = [0, 1];
                self.audio.sample_rate = None;
                self.audio.bit_depth = None;
            }
            Selector::Cue => {
                self.audio.cue_uid = self
                    .devices
                    .iter()
                    .find(|d| optional.as_ref() == Some(&d.name))
                    .and_then(|d| d.uid.clone());
                self.audio.cue_device = optional;
                let master = self
                    .audio
                    .master_device
                    .clone()
                    .unwrap_or_else(|| self.core.engine.snapshot().device_name);
                self.audio.cue_channels = if self.audio.cue_device.as_deref() == Some(&master) {
                    [2, 3]
                } else {
                    [0, 1]
                };
            }
            Selector::MasterChannels | Selector::CueChannels => {
                if let Ok(n) = value.parse::<u16>() {
                    if let Some(next) = n.checked_add(1) {
                        if selector == Selector::MasterChannels {
                            self.audio.master_channels = [n, next];
                        } else {
                            self.audio.cue_channels = [n, next];
                        }
                    }
                }
            }
            Selector::Rate => self.audio.sample_rate = value.parse().ok(),
            Selector::Depth => self.audio.bit_depth = value.parse().ok(),
            Selector::Buffer => self.audio.buffer_frames = value.parse().ok(),
            Selector::MidiChannel => {
                if let Ok(n @ 1..=16) = value.parse::<u8>() {
                    self.binding.channel = n - 1;
                }
            }
            Selector::MidiNumber => {
                if let Ok(n @ 0..=127) = value.parse::<u8>() {
                    self.binding.number = n;
                }
            }
            Selector::MidiMode => {
                if let Some(mode) = value
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| MidiControlMode::ALL.get(i))
                {
                    self.binding.mode = *mode;
                }
            }
            Selector::MidiPreset => {
                self.selected_preset = optional;
                self.controller_advanced = false;
                self.controller_notes = false;
                self.controller_monitor = false;
            }
            Selector::PresetActions => self.run_preset_action(value, cx),
            Selector::ControllerDevice | Selector::Source => {
                self.controller_advanced = false;
                self.controller_notes = false;
                self.controller_monitor = false;
                self.controller_device = optional.clone();
                self.binding.device_id = optional;
                self.selected_preset = self
                    .core
                    .midi
                    .lock()
                    .expect("controllers")
                    .as_ref()
                    .and_then(|hub| {
                        hub.assignments()
                            .into_iter()
                            .find(|a| Some(&a.device_id) == self.controller_device.as_ref())
                            .map(|a| a.preset_id)
                    });
                self.select_midi_target(self.binding.target.clone());
            }
        }
        self.selector = None;
    }

    pub(super) fn apply_midi_preset(&mut self, preset_id: &str, device_id: Option<String>) {
        self.cancel_learn();
        let result = self
            .core
            .midi
            .lock()
            .expect("controllers")
            .as_ref()
            .ok_or("Controllers unavailable".to_string())
            .and_then(|hub| {
                let id = device_id
                    .as_deref()
                    .ok_or("Select the device to apply this preset to".to_string())?;
                hub.apply_preset_to(preset_id, id)
                    .map_err(|e| e.to_string())
            });
        self.result(result, "Preset applied to the selected device");
        self.poll();
    }

    fn reset_defaults(&mut self) {
        let defaults = Settings::default();
        let result = self
            .core
            .settings
            .update(|settings| *settings = defaults.clone());
        if result.is_ok() {
            self.audio = defaults.audio.clone();
            defaults.apply_playback(&self.core.engine);
            self.midi = defaults.midi.clone();
            self.core
                .engine
                .configure_audio(self.audio.clone())
                .map_err(|error| error.to_string())
                .map(|_| ())
                .map_err(|error| {
                    self.status = format!("Defaults saved, audio reset failed: {error}");
                })
                .ok();
        }
        self.result(result, "Defaults restored");
    }
}

fn row(label: &str, control: gpui::AnyElement) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(px(44.))
        .px_3()
        .py_2()
        .border_b_1()
        .border_color(theme::LINE_SOFT)
        .text_size(px(13.))
        .child(div().flex_1().min_w_0().child(label.to_string()))
        .child(control)
}

fn info(label: &str, value: String) -> gpui::Div {
    row(
        label,
        div()
            .max_w(px(370.))
            .text_color(theme::MUTED)
            .child(value)
            .into_any_element(),
    )
}

impl Render for Preferences {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if window.focused(cx).is_none() {
            self.focus.focus(window);
        }
        if self.window_observers.is_empty() {
            self.window_observers
                .push(cx.observe_window_activation(window, |s, window, cx| {
                    if !window.is_window_active() {
                        s.selector = None;
                        cx.notify();
                    }
                }));
            self.window_observers
                .push(cx.observe_window_bounds(window, |s, _, cx| {
                    s.selector = None;
                    cx.notify();
                }));
        }
        self.select_ui.anchors.borrow_mut().clear();
        let status_tip = self.status.clone();
        let content = match self.tab {
            Tab::General => self.general(f32::from(window.viewport_size().width) >= 860., cx),
            Tab::Audio => self.audio_page(cx),
            Tab::Midi => self.midi_page(
                (f32::from(window.viewport_size().width) - 48.).min(1000.),
                cx,
            ),
            Tab::Storage => self.storage_page(cx),
        };
        let mut root = div()
            .id("preferences")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::BG)
            .text_color(theme::TEXT)
            .font_family(theme::FONT_UI)
            .text_size(px(12.))
            .on_key_down(cx.listener(|s, ev: &gpui::KeyDownEvent, window, cx| {
                if s.confirm_reset {
                    if ev.keystroke.key == "escape" {
                        s.confirm_reset = false;
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                if s.import_preview.is_some() {
                    if ev.keystroke.key == "escape" {
                        s.import_preview = None;
                        cx.notify();
                    }
                    cx.stop_propagation();
                    return;
                }
                if s.preset_dialog.is_some() {
                    s.preset_key(&ev.keystroke, cx);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if s.selector.is_some() && s.select_key(&ev.keystroke, cx) {
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if ev.keystroke.key == "escape" {
                    if s.selector.is_some() {
                        s.selector = None;
                    } else if s.confirm_clear.is_some() {
                        s.confirm_clear = None;
                    } else if s.learning {
                        s.cancel_learn();
                    } else {
                        s.cancel_learn();
                        window.remove_window();
                    }
                    cx.stop_propagation();
                    cx.notify();
                } else if ev.keystroke.modifiers.platform && ev.keystroke.key == "w" {
                    s.cancel_learn();
                    window.remove_window();
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .h(px(TITLEBAR_HEIGHT))
                    .pl(px(96.))
                    .pr_6()
                    .bg(theme::PANEL)
                    .text_size(px(13.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .child("Preferences"),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap_6()
                    .h(px(48.))
                    .px_6()
                    .bg(theme::PANEL)
                    .children(
                        [
                            (Tab::General, "General"),
                            (Tab::Audio, "Audio I/O"),
                            (Tab::Midi, "Controllers"),
                            (Tab::Storage, "Storage"),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(index, (tab, label))| {
                            let active = self.tab == tab;
                            div()
                                .id(("tab", index))
                                .debug_selector(move || format!("tab-{label}"))
                                .h_full()
                                .px_1()
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(13.))
                                .text_color(if active { theme::TEXT } else { theme::MUTED })
                                .when(active, |el| {
                                    el.font_weight(gpui::FontWeight::MEDIUM)
                                        .border_b_2()
                                        .border_color(theme::ACCENT)
                                })
                                .hover(|el| el.text_color(theme::TEXT))
                                .child(label)
                                .on_click(cx.listener(move |s, _, _, cx| {
                                    s.cancel_learn();
                                    s.tab = tab;
                                    s.selector = None;
                                    s.confirm_clear = None;
                                    if tab == Tab::Storage && s.storage.is_none() {
                                        s.refresh_storage();
                                    }
                                    cx.notify();
                                }))
                        }),
                    ),
            )
            .child(
                div()
                    .id(("preferences-scroll", self.tab as usize))
                    .debug_selector(|| "settings-content".into())
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_6()
                    .pb_6()
                    .pt_5()
                    .child(content),
            )
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .h(px(64.))
                    .px_6()
                    .bg(theme::PANEL)
                    .border_t_1()
                    .border_color(theme::LINE)
                    .child(self.button(
                        "reset-defaults",
                        "Reset defaults",
                        self.job.is_none(),
                        |s, _, _| {
                            s.selector = None;
                            s.confirm_reset = true;
                        },
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .justify_end()
                            .gap_3()
                            .child(
                                div()
                                    .id("settings-status")
                                    .tooltip(move |_, cx| {
                                        cx.new(|_| crate::views::TextTip(status_tip.clone())).into()
                                    })
                                    .flex_1()
                                    .min_w_0()
                                    .max_h(px(40.))
                                    .overflow_hidden()
                                    .text_size(px(11.))
                                    .text_color(if self.error {
                                        theme::DANGER
                                    } else {
                                        theme::MUTED
                                    })
                                    .when(!self.status.is_empty(), |el| {
                                        el.child(self.status.clone())
                                    }),
                            )
                            .child(
                                self.primary_button(
                                    "close-preferences",
                                    "Done",
                                    true,
                                    |s, window, _| {
                                        s.cancel_learn();
                                        window.remove_window();
                                    },
                                    cx,
                                )
                                .min_w(px(80.))
                                .font_weight(gpui::FontWeight::MEDIUM),
                            ),
                    ),
            );
        if self.confirm_reset {
            root = root.child(self.reset_dialog(cx));
        }
        if self.preset_dialog.is_some() || self.import_preview.is_some() {
            root = root.child(self.controller_dialog(
                f32::from(window.viewport_size().height),
                window,
                cx,
            ));
        }
        if let Some(selector) = self.selector {
            root = root.child(self.select_menu(selector, window, cx));
        }
        root
    }
}

pub fn open(core: Arc<AppCore>, cx: &mut App) {
    for window in cx.windows() {
        if let Some(window) = window.downcast::<Preferences>() {
            if window
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return;
            }
        }
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(940.), px(820.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("mixless Preferences".into()),
            appears_transparent: true,
            // AppKit's 14-point controls share the title row's vertical center.
            traffic_light_position: Some(gpui::point(px(16.), px((TITLEBAR_HEIGHT - 14.) / 2.))),
            ..Default::default()
        }),
        window_min_size: Some(size(px(720.), px(540.))),
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |_, cx| cx.new(|cx| Preferences::new(core, cx))) {
        tracing::error!("open preferences: {error}");
    }
}
