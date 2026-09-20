use std::path::PathBuf;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use gpui::{
    App, Bounds, Context, FocusHandle, IntoElement, Render, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use mixless_engine::{AudioConfig, AudioDevice};
use mixless_midi::{
    MidiBinding, MidiConfig, MidiControlMode, MidiGroup, MidiMessage, MidiPort, MidiSourceKind,
    MidiTarget,
};
use mixless_protocol::{Command, DeckId, XfCurve};

use crate::{
    settings::Settings,
    state::{AppCore, WaveLayout},
    storage::{self, ClearKind, StorageReport},
    theme,
};

mod midi;

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    General,
    Audio,
    Midi,
    Storage,
}

#[derive(Clone, Copy, PartialEq)]
enum Selector {
    Master,
    Cue,
    Rate,
    Buffer,
    Source,
    MidiChannel,
    MidiNumber,
    MidiMode,
}

enum JobResult {
    Devices(
        Result<Vec<AudioDevice>, String>,
        Result<Vec<MidiPort>, String>,
    ),
    Audio(Result<(), String>),
    Midi(Result<(), String>),
    File(Result<(), String>),
    Dir(Option<PathBuf>),
    Storage(Result<StorageReport, String>),
    StorageClear(Result<(u64, StorageReport), String>),
}

pub struct Preferences {
    core: Arc<AppCore>,
    focus: FocusHandle,
    tab: Tab,
    selector: Option<Selector>,
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
    last: Option<MidiMessage>,
    job: Option<mpsc::Receiver<JobResult>>,
    status: String,
    error: bool,
    storage: Option<StorageReport>,
    confirm_clear: Option<ClearKind>,
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
            last: None,
            job: None,
            error: !status.is_empty(),
            status,
            storage: None,
            confirm_clear: None,
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
                mixless_midi::input_ports().map_err(|e| e.to_string()),
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
                Ok(JobResult::File(result)) => self.result(result, "MIDI mapping file updated"),
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
                        mixless_midi::MidiHub::start_with_config(path, config.clone())
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
                *core.midi_error.lock().expect("MIDI error") = None;
                Ok(())
            })())
        });
    }

    fn cancel_learn(&mut self) {
        self.learning = false;
        if let Some(hub) = self.core.midi.lock().expect("MIDI").as_ref() {
            hub.set_learn(false);
        }
    }

    fn toggle_learn(&mut self) {
        self.learning = !self.learning;
        if let Some(hub) = self.core.midi.lock().expect("MIDI").as_ref() {
            hub.set_learn(self.learning);
        }
    }

    fn save_binding(&mut self) {
        let result = {
            let midi = self.core.midi.lock().expect("MIDI");
            midi.as_ref()
                .ok_or_else(|| "MIDI is unavailable".to_string())
                .and_then(|hub| {
                    if self.learning && hub.learned().is_none() {
                        return Err("Waiting for a MIDI signal".into());
                    }
                    if let Some(index) = self.editing {
                        hub.replace(index, self.binding.clone())
                            .map_err(|e| e.to_string())
                    } else {
                        hub.upsert(self.binding.clone()).map_err(|e| e.to_string())
                    }
                })
        };
        if result.is_ok() {
            self.cancel_learn();
        }
        self.result(result, "MIDI mapping saved");
        self.poll();
    }

    fn file_dialog(&mut self, export: bool, cx: &mut Context<Self>) {
        if self.job.is_some() {
            return;
        }
        self.cancel_learn();
        let (tx, rx) = mpsc::channel();
        self.job = Some(rx);
        let core = self.core.clone();
        cx.spawn(async move |_, _| {
            let dialog = rfd::AsyncFileDialog::new().add_filter("MIDI mapping", &["json"]);
            let file = if export {
                dialog.set_file_name("mixless-midi.json").save_file().await
            } else {
                dialog.pick_file().await
            };
            let result = match file {
                Some(file) => {
                    let midi = core.midi.lock().expect("MIDI");
                    midi.as_ref()
                        .ok_or_else(|| "MIDI is unavailable".to_string())
                        .and_then(|hub| {
                            if export {
                                hub.export(file.path())
                            } else {
                                hub.import(file.path())
                            }
                            .map_err(|e| e.to_string())
                        })
                }
                None => Ok(()),
            };
            let _ = tx.send(JobResult::File(result));
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
            .min_h(px(44.))
            .py_2()
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
                            .text_color(if armed {
                                theme::DANGER
                            } else {
                                theme::TEXT
                            }),
                        )
                    }),
            )
    }

    fn storage_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let busy = self.job.is_some();
        let mut page = div().flex().flex_col().child(section("Cached data")).child(
            div()
                .pb_2()
                .text_size(px(11.))
                .text_color(theme::MUTED)
                .child(
                    "Derived data Mixless recomputes on demand. Clearing never removes \
                     source audio, playlists or saved cues."
                        .to_string(),
                ),
        );
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
            .child(self.storage_row(
                "stems",
                "Stem cache",
                "Separated vocals, drums and instruments used by stem mixing",
                report.stems,
                Some(ClearKind::Stems),
                cx,
            ))
            .child(self.storage_row(
                "analysis",
                "Track analysis",
                "Beat grids, keys, structure and note evidence in library.db",
                report.analysis,
                Some(ClearKind::Analysis),
                cx,
            ))
            .child(self.storage_row(
                "waveforms",
                "Waveforms",
                "Spectral envelopes in library.db, rebuilt from decoded audio",
                report.waveforms,
                Some(ClearKind::Waveforms),
                cx,
            ))
            .child(self.storage_row(
                "artwork",
                "Artwork",
                "Extracted covers, re-extracted on next launch",
                report.artwork,
                Some(ClearKind::Artwork),
                cx,
            ))
            .child(self.storage_row(
                "models",
                "Analysis models",
                "ONNX models, downloaded again when stem analysis runs",
                report.models,
                Some(ClearKind::Models),
                cx,
            ))
            .child(section("Library"))
            .child(self.storage_row(
                "database",
                "Library database",
                "Tracks, cues, playlists and analysis payloads",
                report.database,
                None,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .min_h(px(44.))
                    .py_2()
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
                                div()
                                    .text_size(px(11.))
                                    .text_color(theme::MUTED)
                                    .child(crate::state::display_dir(&self.core.download_dir())),
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
            );
        if !report.playlists.is_empty() {
            page = page.child(section("Per playlist")).child(
                div()
                    .pb_2()
                    .text_size(px(11.))
                    .text_color(theme::MUTED)
                    .child(
                        "Clear stems, analysis, waveforms and artwork for the tracks in a list. \
                         Caches are per track, so tracks shared with other lists are cleared too."
                            .to_string(),
                    ),
            );
            for playlist in &report.playlists {
                page = page.child(self.storage_row(
                    format!("playlist-{}", playlist.id),
                    &playlist.name,
                    &format!("{} tracks", playlist.tracks),
                    playlist.bytes,
                    Some(ClearKind::Playlist(playlist.id)),
                    cx,
                ));
            }
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
        div()
            .id(id.into())
            .h(px(30.))
            .px_3()
            .flex()
            .items_center()
            .justify_center()
            .flex_none()
            .rounded(px(4.))
            .border_1()
            .border_color(theme::LINE)
            .bg(theme::PANEL_RAISED)
            .text_size(px(12.))
            .text_color(if enabled { theme::TEXT } else { theme::MUTED })
            .when(enabled, |el| {
                el.cursor_pointer().hover(|el| el.bg(theme::LINE))
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
            .w(px(38.))
            .h(px(22.))
            .px(px(3.))
            .flex()
            .items_center()
            .flex_none()
            .rounded_full()
            .bg(if on { theme::LED_GREEN } else { theme::LINE })
            .when(on, |el| el.justify_end())
            .cursor_pointer()
            .child(div().size(px(16.)).rounded_full().bg(theme::TEXT))
            .on_click(cx.listener(move |s, _, _, cx| {
                action(s);
                cx.notify();
            }))
            .into_any_element()
    }

    fn select(&self, selector: Selector, value: String, cx: &Context<Self>) -> gpui::AnyElement {
        self.button(
            format!("select-{}", selector as u8),
            value,
            self.job.is_none(),
            move |s, _, _| {
                s.selector = Some(selector);
            },
            cx,
        )
        .w(px(340.))
        .h_auto()
        .min_h(px(30.))
        .py_2()
        .gap_2()
        .justify_between()
        .child(div().flex_none().child("\u{2304}"))
        .into_any_element()
    }

    fn general(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let settings = self.core.settings.get();
        let mut page = div()
            .flex()
            .flex_col()
            .child(section("Appearance"))
            .child(row(
                "Waveform layout",
                div()
                    .flex()
                    .gap_1()
                    .children(
                        [(WaveLayout::Top, "Top"), (WaveLayout::Center, "Center")].map(
                            |(layout, label)| {
                                self.button(
                                    format!("layout-{label}"),
                                    label,
                                    true,
                                    move |s, _, _| s.save_general(|p| p.wave_layout = layout),
                                    cx,
                                )
                                .text_color(
                                    if settings.wave_layout == layout {
                                        theme::ACCENT
                                    } else {
                                        theme::MUTED
                                    },
                                )
                            },
                        ),
                    )
                    .into_any_element(),
            ))
            .child(row(
                "Effects panel",
                self.toggle(
                    "show-fx",
                    settings.show_fx,
                    |s| s.save_general(|p| p.show_fx = !p.show_fx),
                    cx,
                ),
            ))
            .child(section("Playback"));
        for (id, label, on, field) in [
            ("quantize", "Quantize", settings.quantize, 0),
            ("fx-auto-fade", "FX fade in / out", settings.fx_auto_fade, 6),
            ("keylock", "Key lock", settings.keylock, 1),
            ("vinyl", "Vinyl mode", settings.vinyl, 2),
            ("slip", "Slip mode", settings.slip, 3),
            (
                "filter-resonance",
                "Filter / EQ resonance",
                settings.filter_resonance,
                5,
            ),
            ("xf-reverse", "Crossfader reverse", settings.xf_reverse, 4),
        ] {
            page = page.child(row(
                label,
                self.toggle(
                    id,
                    on,
                    move |s| {
                        s.save_general(|p| match field {
                            0 => p.quantize = !p.quantize,
                            1 => p.keylock = !p.keylock,
                            2 => p.vinyl = !p.vinyl,
                            3 => p.slip = !p.slip,
                            5 => p.filter_resonance = !p.filter_resonance,
                            6 => p.fx_auto_fade = !p.fx_auto_fade,
                            _ => p.xf_reverse = !p.xf_reverse,
                        })
                    },
                    cx,
                ),
            ));
        }
        page.child(row(
            "Crossfader curve",
            div()
                .flex()
                .gap_1()
                .children(
                    [
                        (XfCurve::Linear, "Linear"),
                        (XfCurve::EqualPower, "Equal power"),
                        (XfCurve::Cut, "Cut"),
                        (XfCurve::Scratch, "Scratch"),
                    ]
                    .map(|(curve, label)| {
                        self.button(
                            format!("curve-{label}"),
                            label,
                            true,
                            move |s, _, _| s.save_general(|p| p.xf_curve = curve),
                            cx,
                        )
                        .text_color(if settings.xf_curve == curve {
                            theme::ACCENT
                        } else {
                            theme::MUTED
                        })
                    }),
                )
                .into_any_element(),
        ))
        .child(section("Library"))
        .child(row(
            "Stem and note analysis for AutoMix",
            self.toggle(
                "deep-analysis",
                settings.deep_analysis,
                |s| s.save_general(|p| p.deep_analysis = !p.deep_analysis),
                cx,
            ),
        ))
        .child(info(
            "Analysis",
            "Runs locally in the background. First use downloads models; playback stays available."
                .into(),
        ))
        .child(row(
            "Library and preferences",
            self.button(
                "data-folder",
                "Show in Finder",
                true,
                |s, _, cx| {
                    cx.reveal_path(s.core.settings.path.parent().unwrap_or(&PathBuf::from(".")))
                },
                cx,
            )
            .into_any_element(),
        ))
        .child(row(
            "Downloaded audio",
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    self.button(
                        "audio-folder",
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
                )
                .child(
                    self.button(
                        "audio-folder-change",
                        "Change…",
                        self.job.is_none(),
                        |s, _, cx| s.pick_download_dir(cx),
                        cx,
                    )
                    .into_any_element(),
                )
                .when(settings.download_dir.is_some(), |el| {
                    el.child(
                        self.button(
                            "audio-folder-default",
                            "Use default",
                            self.job.is_none(),
                            |s, _, _| s.save_general(|p| p.download_dir = None),
                            cx,
                        )
                        .into_any_element(),
                    )
                })
                .into_any_element(),
        ))
        .child(info(
            "Location",
            crate::state::display_dir(&self.core.download_dir()),
        ))
        .child(info(
            "Spotify playlists",
            "New downloads are saved in a subfolder named after the playlist.".into(),
        ))
        .into_any_element()
    }

    fn audio_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let snapshot = self.core.engine.snapshot();
        let settings = self.core.settings.get();
        let available = self.core.engine.audio_available();
        div()
            .flex()
            .flex_col()
            .child(section("Output routing"))
            .child(row(
                "Master output",
                self.select(
                    Selector::Master,
                    self.audio
                        .master_device
                        .clone()
                        .unwrap_or("System Default".into()),
                    cx,
                ),
            ))
            .child(row(
                "Headphone output (PFL)",
                self.select(
                    Selector::Cue,
                    self.audio.cue_device.clone().unwrap_or("Disabled".into()),
                    cx,
                ),
            ))
            .child(row(
                "Sample rate",
                self.select(
                    Selector::Rate,
                    self.audio
                        .sample_rate
                        .map_or("Device native".into(), |rate| format!("{rate} Hz")),
                    cx,
                ),
            ))
            .child(row(
                "Buffer size",
                self.select(
                    Selector::Buffer,
                    self.audio
                        .buffer_frames
                        .map_or("Device default".into(), |frames| format!("{frames} frames")),
                    cx,
                ),
            ))
            .child(row(
                "Headphone volume",
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(self.button(
                        "cue-down",
                        "-",
                        settings.cue_gain > 0.0,
                        |s, _, _| s.save_general(|p| p.cue_gain = (p.cue_gain - 0.05).max(0.0)),
                        cx,
                    ))
                    .child(
                        div()
                            .w(px(45.))
                            .text_center()
                            .child(format!("{:.0}%", settings.cue_gain * 100.0)),
                    )
                    .child(self.button(
                        "cue-up",
                        "+",
                        settings.cue_gain < 1.0,
                        |s, _, _| s.save_general(|p| p.cue_gain = (p.cue_gain + 0.05).min(1.0)),
                        cx,
                    ))
                    .into_any_element(),
            ))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .py_4()
                    .child(self.button(
                        "refresh-audio",
                        "Refresh devices",
                        self.job.is_none(),
                        |s, _, _| s.refresh(),
                        cx,
                    ))
                    .child(self.button(
                        "revert-audio",
                        "Revert",
                        self.job.is_none(),
                        |s, _, _| s.audio = s.core.settings.get().audio,
                        cx,
                    ))
                    .child(
                        self.button(
                            "apply-audio",
                            "Apply audio",
                            self.job.is_none(),
                            |s, _, _| s.apply_audio(),
                            cx,
                        )
                        .text_color(theme::ACCENT),
                    ),
            )
            .child(section("Current audio"))
            .child(info(
                "Status",
                if available {
                    "Running".into()
                } else {
                    "Audio unavailable".into()
                },
            ))
            .child(info("Master", snapshot.device_name))
            .child(info(
                "Headphones",
                snapshot.cue_device.unwrap_or("Disabled".into()),
            ))
            .child(info(
                "Sample rate / buffer",
                format!(
                    "{} Hz / {} frames",
                    snapshot.sample_rate, snapshot.block_frames
                ),
            ))
            .child(info(
                "Buffer duration",
                format!(
                    "{:.2} ms",
                    snapshot.block_frames as f64 * 1000.0 / snapshot.sample_rate.max(1) as f64
                ),
            ))
            .child(info("Underruns", snapshot.xrun_count.to_string()))
            .when(self.audio != self.core.engine.audio_config(), |el| {
                el.child(
                    div()
                        .py_3()
                        .text_color(theme::WARN)
                        .text_size(px(12.))
                        .child("Selected configuration differs from the active audio device."),
                )
            })
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
                        "System Default"
                    } else {
                        "Disabled"
                    }
                    .into(),
                )];
                let master = self
                    .audio
                    .master_device
                    .clone()
                    .unwrap_or_else(|| self.core.engine.snapshot().device_name);
                options.extend(
                    self.devices
                        .iter()
                        .filter(|d| selector == Selector::Master || d.name != master)
                        .map(|d| (d.name.clone(), d.name.clone())),
                );
                options
            }
            Selector::Rate => {
                let mut options = vec![(String::new(), "Device native".into())];
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
                    .unwrap_or(&[44_100, 48_000, 88_200, 96_000]);
                options.extend(
                    rates
                        .iter()
                        .map(|rate| (rate.to_string(), format!("{rate} Hz"))),
                );
                options
            }
            Selector::Buffer => {
                let mut options = vec![(String::new(), "Device default".into())];
                options.extend(
                    [64, 128, 256, 512, 1024, 2048].map(|n| (n.to_string(), format!("{n} frames"))),
                );
                options
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
            Selector::Source => {
                let mut options = vec![(String::new(), "Any enabled input".into())];
                options.extend(self.ports.iter().map(|p| (p.id.clone(), p.name.clone())));
                options
            }
        }
    }

    fn choose(&mut self, selector: Selector, value: &str) {
        let optional = (!value.is_empty()).then(|| value.to_string());
        match selector {
            Selector::Master => {
                self.audio.master_device = optional;
                self.audio.sample_rate = None;
                if self.audio.cue_device == self.audio.master_device {
                    self.audio.cue_device = None;
                }
            }
            Selector::Cue => self.audio.cue_device = optional,
            Selector::Rate => self.audio.sample_rate = value.parse().ok(),
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
            Selector::Source => self.binding.device_id = optional,
        }
        self.selector = None;
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

fn section(label: &str) -> gpui::Div {
    div()
        .pt_4()
        .pb_2()
        .text_size(px(14.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .child(label.to_string())
}

fn row(label: &str, control: gpui::AnyElement) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .min_h(px(44.))
        .py_2()
        .border_b_1()
        .border_color(theme::LINE_SOFT)
        .text_size(px(12.))
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
        let content = match self.tab {
            Tab::General => self.general(cx),
            Tab::Audio => self.audio_page(cx),
            Tab::Midi => self.midi_page(cx),
            Tab::Storage => self.storage_page(cx),
        };
        let mut root = div()
            .id("preferences")
            .track_focus(&self.focus)
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme::PANEL)
            .text_color(theme::TEXT)
            .font_family(theme::FONT_UI)
            .text_size(px(12.))
            .on_key_down(cx.listener(|s, ev: &gpui::KeyDownEvent, window, cx| {
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
                    .gap_2()
                    // The transparent macOS titlebar overlays this row. Keep
                    // the tabs clear of the native traffic-light controls.
                    .pl(px(96.))
                    .pr_6()
                    .py_3()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .children(
                        [
                            (Tab::General, "General"),
                            (Tab::Audio, "Audio I/O"),
                            (Tab::Midi, "MIDI Mapping"),
                            (Tab::Storage, "Storage"),
                        ]
                        .map(|(tab, label)| {
                            self.button(
                                format!("tab-{label}"),
                                label,
                                true,
                                move |s, _, _| {
                                    s.cancel_learn();
                                    s.tab = tab;
                                    s.selector = None;
                                    s.confirm_clear = None;
                                    if tab == Tab::Storage && s.storage.is_none() {
                                        s.refresh_storage();
                                    }
                                },
                                cx,
                            )
                            .text_color(if self.tab == tab {
                                theme::ACCENT
                            } else {
                                theme::MUTED
                            })
                        }),
                    ),
            )
            .child(
                div()
                    .id(("preferences-scroll", self.tab as usize))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_6()
                    .pb_4()
                    .child(content),
            )
            .child(
                div()
                    .flex_none()
                    .min_h(px(58.))
                    .items_center()
                    .justify_between()
                    .px_6()
                    .py_3()
                    .border_t_1()
                    .border_color(theme::LINE)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(px(11.))
                            .text_color(if self.error {
                                theme::DANGER
                            } else {
                                theme::MUTED
                            })
                            .child(if self.job.is_some() && self.status.is_empty() {
                                "Loading devices...".into()
                            } else {
                                self.status.clone()
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .child(self.button(
                                "reset-defaults",
                                "Reset defaults",
                                self.job.is_none(),
                                |s, _, _| s.reset_defaults(),
                                cx,
                            ))
                            .child(
                                self.button(
                                    "close-preferences",
                                    "Close",
                                    true,
                                    |s, window, _| {
                                        s.cancel_learn();
                                        window.remove_window();
                                    },
                                    cx,
                                )
                                .text_color(theme::ACCENT),
                            ),
                    ),
            );
        if let Some(selector) = self.selector {
            root = root.child(
                div()
                    .absolute()
                    .top(px(72.))
                    .right(px(24.))
                    .w(px(500.))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(|s, _, _, cx| {
                            s.selector = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .id("preference-options")
                            .max_h(px(430.))
                            .overflow_y_scroll()
                            .p(px(3.))
                            .rounded(px(theme::POPUP_RADIUS))
                            .bg(theme::PANEL_RAISED)
                            .border_1()
                            .border_color(theme::LINE)
                            .shadow_sm()
                            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| {
                                cx.stop_propagation()
                            })
                            .children(self.selector_options(selector).into_iter().enumerate().map(
                                |(index, (value, label))| {
                                    self.button(
                                        format!("option-{index}"),
                                        label,
                                        true,
                                        move |s, _, _| s.choose(selector, &value),
                                        cx,
                                    )
                                    .w_full()
                                    .h_auto()
                                    .min_h(px(theme::MENU_ROW_HEIGHT))
                                    .px(px(8.))
                                    .py(px(3.))
                                    .text_size(px(11.))
                                    .line_height(px(16.))
                                    .rounded(px(2.))
                                    .justify_start()
                                    .border_0()
                                },
                            )),
                    ),
            );
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
            size(px(760.), px(760.)),
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some("Mixless Preferences".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(16.), px(18.))),
            ..Default::default()
        }),
        window_min_size: Some(size(px(720.), px(540.))),
        ..Default::default()
    };
    if let Err(error) = cx.open_window(options, |_, cx| cx.new(|cx| Preferences::new(core, cx))) {
        tracing::error!("open preferences: {error}");
    }
}
