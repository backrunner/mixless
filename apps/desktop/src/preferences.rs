use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use gpui::{
    div, prelude::*, px, size, App, Bounds, Context, FocusHandle, IntoElement, Render,
    SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions,
};
use mixless_engine::{AudioConfig, AudioDevice};
use mixless_midi::{MidiBinding, MidiConfig, MidiMessage, MidiPort, MidiSourceKind, MidiTarget};
use mixless_protocol::{Command, DeckId, XfCurve};

use crate::{
    settings::Settings,
    state::{AppCore, WaveLayout},
    theme,
};

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    General,
    Audio,
    Midi,
}

#[derive(Clone, Copy, PartialEq)]
enum Selector {
    Master,
    Cue,
    Rate,
    Buffer,
    Target,
    Source,
}

enum JobResult {
    Devices(
        Result<Vec<AudioDevice>, String>,
        Result<Vec<MidiPort>, String>,
    ),
    Audio(Result<(), String>),
    Midi(Result<(), String>),
    File(Result<(), String>),
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
    learning: bool,
    last: Option<MidiMessage>,
    job: Option<mpsc::Receiver<JobResult>>,
    status: String,
    error: bool,
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
            },
            editing: None,
            learning: false,
            last: None,
            job: None,
            error: !status.is_empty(),
            status,
        };
        state.refresh();
        cx.on_release(|state, _| state.cancel_learn()).detach();
        cx.spawn(async move |this, cx| loop {
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
                Err(mpsc::TryRecvError::Empty) => self.job = Some(rx),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.result(Err("Settings operation stopped unexpectedly".into()), "")
                }
            }
        }
        if let Some(hub) = self.core.midi.lock().expect("MIDI").as_ref() {
            self.bindings = hub.bindings();
            self.last = hub.last_message();
            if self.learning {
                if let Some(message) = hub.learned() {
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
            self.editing = None;
        }
        self.result(result, "MIDI mapping saved");
        self.poll();
    }

    fn file_dialog(&mut self, export: bool, cx: &mut Context<Self>) {
        if self.job.is_some() {
            return;
        }
        self.cancel_learn();
        self.editing = None;
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
            self.button(
                "audio-folder",
                "Show in Finder",
                true,
                |s, _, cx| cx.reveal_path(&s.core.acquired_dir),
                cx,
            )
            .into_any_element(),
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

    fn midi_page(&self, cx: &Context<Self>) -> gpui::AnyElement {
        let busy = self.job.is_some();
        let midi_ready = self.core.midi.lock().expect("MIDI").is_some();
        let mut page = div()
            .flex()
            .flex_col()
            .child(section("MIDI inputs"))
            .child(row(
                "Enable MIDI",
                self.toggle(
                    "midi-enabled",
                    self.midi.enabled,
                    |s| s.midi.enabled = !s.midi.enabled,
                    cx,
                ),
            ))
            .child(row(
                "All input devices",
                self.toggle(
                    "midi-all",
                    self.midi.input_ids.is_none(),
                    |s| {
                        s.midi.input_ids = if s.midi.input_ids.is_none() {
                            Some(Vec::new())
                        } else {
                            None
                        };
                    },
                    cx,
                ),
            ));
        for port in &self.ports {
            let id = port.id.clone();
            let selected = self
                .midi
                .input_ids
                .as_ref()
                .is_none_or(|ids| ids.contains(&id));
            let toggle = self
                .button(
                    format!("port-{id}"),
                    if selected { "\u{2713}" } else { " " },
                    !busy,
                    move |s, _, _| {
                        let ids = s
                            .midi
                            .input_ids
                            .get_or_insert_with(|| s.ports.iter().map(|p| p.id.clone()).collect());
                        if ids.contains(&id) {
                            ids.retain(|p| p != &id);
                        } else {
                            ids.push(id.clone());
                        }
                    },
                    cx,
                )
                .into_any_element();
            page = page.child(row(&port.name, toggle));
        }
        if let Some(ids) = &self.midi.input_ids {
            for id in ids
                .iter()
                .filter(|id| !self.ports.iter().any(|p| &p.id == *id))
            {
                let id = id.clone();
                page = page.child(row(
                    &format!("Disconnected input: {id}"),
                    self.button(
                        format!("missing-{id}"),
                        "Remove",
                        !busy,
                        move |s, _, _| {
                            if let Some(ids) = &mut s.midi.input_ids {
                                ids.retain(|p| p != &id);
                            }
                        },
                        cx,
                    )
                    .into_any_element(),
                ));
            }
        }
        page = page
            .when(self.ports.is_empty(), |el| {
                el.child(info("Inputs", "No MIDI devices connected".into()))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .py_3()
                    .child(self.button(
                        "refresh-midi",
                        "Refresh devices",
                        !busy,
                        |s, _, _| s.refresh(),
                        cx,
                    ))
                    .child(
                        self.button(
                            "apply-midi",
                            "Apply inputs",
                            !busy,
                            |s, _, _| s.apply_midi(),
                            cx,
                        )
                        .text_color(theme::ACCENT),
                    ),
            )
            .child(section(if self.editing.is_some() {
                "Edit mapping"
            } else {
                "New mapping"
            }))
            .child(row(
                "Action",
                self.select(Selector::Target, self.binding.target.label(), cx),
            ))
            .child(row(
                "Source device",
                self.select(
                    Selector::Source,
                    self.binding
                        .device_id
                        .as_ref()
                        .map_or("Any enabled input".into(), |id| self.port_name(id)),
                    cx,
                ),
            ))
            .child(row(
                "Message",
                div()
                    .flex()
                    .gap_2()
                    .children(
                        [(MidiSourceKind::Cc, "CC"), (MidiSourceKind::Note, "Note")].map(
                            |(kind, label)| {
                                self.button(
                                    format!("kind-{label}"),
                                    label,
                                    !busy && !self.learning,
                                    move |s, _, _| s.binding.kind = kind,
                                    cx,
                                )
                                .text_color(
                                    if self.binding.kind == kind {
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
                "Channel",
                self.number_stepper("channel", self.binding.channel + 1, 1, 16, cx),
            ))
            .child(row(
                "Controller / note",
                self.number_stepper("number", self.binding.number, 0, 127, cx),
            ))
            .child(info(
                "Last signal",
                self.last.as_ref().map_or("No signal".into(), |m| {
                    format!(
                        "{} / {:?} {} / Ch {} / {}",
                        self.port_name(&m.device_id),
                        m.kind,
                        m.number,
                        m.channel + 1,
                        m.value
                    )
                }),
            ))
            .when(self.learning, |el| {
                el.child(
                    div().py_2().text_color(theme::ACCENT).child(
                        if self
                            .core
                            .midi
                            .lock()
                            .expect("MIDI")
                            .as_ref()
                            .and_then(|h| h.learned())
                            .is_some()
                        {
                            "Signal captured"
                        } else {
                            "Waiting for MIDI..."
                        },
                    ),
                )
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .py_3()
                    .child(self.button(
                        "learn",
                        if self.learning {
                            "Cancel learn"
                        } else {
                            "Learn"
                        },
                        midi_ready && !busy,
                        |s, _, _| s.toggle_learn(),
                        cx,
                    ))
                    .when(self.editing.is_some(), |el| {
                        el.child(self.button(
                            "cancel-edit",
                            "Cancel edit",
                            !busy,
                            |s, _, _| {
                                s.cancel_learn();
                                s.editing = None;
                            },
                            cx,
                        ))
                    })
                    .child(
                        self.button(
                            "save-binding",
                            "Save mapping",
                            midi_ready && !busy,
                            |s, _, _| s.save_binding(),
                            cx,
                        )
                        .text_color(theme::ACCENT),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pt_4()
                    .pb_2()
                    .child(
                        div()
                            .text_size(px(14.))
                            .child(format!("Mappings ({})", self.bindings.len())),
                    )
                    .child(
                        div()
                            .flex()
                            .gap_2()
                            .child(self.button(
                                "import-map",
                                "Import...",
                                midi_ready && !busy,
                                |s, _, cx| s.file_dialog(false, cx),
                                cx,
                            ))
                            .child(self.button(
                                "export-map",
                                "Export...",
                                midi_ready && !busy,
                                |s, _, cx| s.file_dialog(true, cx),
                                cx,
                            )),
                    ),
            );
        for (index, binding) in self.bindings.iter().enumerate() {
            page = page.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .py_2()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(div().text_size(px(12.)).child(binding.target.label()))
                            .child(div().text_size(px(11.)).text_color(theme::MUTED).child(
                                format!(
                                        "{} / {:?} {} / Ch {}",
                                        binding
                                            .device_id
                                            .as_ref()
                                            .map_or("Any input".into(), |id| self.port_name(id)),
                                        binding.kind,
                                        binding.number,
                                        binding.channel + 1
                                    ),
                            )),
                    )
                    .child(self.button(
                        format!("edit-{index}"),
                        "Edit",
                        !busy,
                        move |s, _, _| {
                            s.cancel_learn();
                            s.binding = s.bindings[index].clone();
                            s.editing = Some(index);
                        },
                        cx,
                    ))
                    .child(self.button(
                        format!("delete-{index}"),
                        "Delete",
                        !busy,
                        move |s, _, _| {
                            s.cancel_learn();
                            s.editing = None;
                            let result = s
                                .core
                                .midi
                                .lock()
                                .expect("MIDI")
                                .as_ref()
                                .unwrap()
                                .remove(index)
                                .map_err(|e| e.to_string());
                            s.result(result, "Mapping deleted");
                            s.poll();
                        },
                        cx,
                    )),
            );
        }
        if self.bindings.is_empty() {
            page = page.child(info("Mappings", "No mappings".into()));
        }
        if let Some(error) = self.core.midi_error.lock().expect("MIDI error").as_ref() {
            page = page.child(div().py_3().text_color(theme::DANGER).child(error.clone()));
        }
        page.into_any_element()
    }

    fn number_stepper(
        &self,
        field: &'static str,
        value: u8,
        min: u8,
        max: u8,
        cx: &Context<Self>,
    ) -> gpui::AnyElement {
        let step = |delta: i16, label: &'static str| {
            self.button(
                format!("{field}-{label}"),
                label,
                !self.learning
                    && self.job.is_none()
                    && if delta < 0 { value > min } else { value < max },
                move |s, _, _| {
                    let value = (value as i16 + delta).clamp(min as i16, max as i16) as u8;
                    if field == "channel" {
                        s.binding.channel = value - 1;
                    } else {
                        s.binding.number = value;
                    }
                },
                cx,
            )
        };
        div()
            .flex()
            .items_center()
            .gap_3()
            .child(step(-1, "-"))
            .child(div().w(px(36.)).text_center().child(value.to_string()))
            .child(step(1, "+"))
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
            Selector::Target => MidiTarget::all()
                .into_iter()
                .enumerate()
                .map(|(i, t)| (i.to_string(), t.label()))
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
            Selector::Target => {
                if let Ok(index) = value.parse::<usize>() {
                    if let Some(target) = MidiTarget::all().get(index) {
                        self.binding.target = target.clone();
                    }
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
                    .items_center()
                    .gap_2()
                    .px_6()
                    .py_3()
                    .border_b_1()
                    .border_color(theme::LINE)
                    .children(
                        [
                            (Tab::General, "General"),
                            (Tab::Audio, "Audio I/O"),
                            (Tab::Midi, "MIDI Mapping"),
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
