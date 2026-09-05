//! Shared app core + UI state. The engine/library live behind `Arc` so
//! background threads can import and load tracks without blocking the UI.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use gpui::{Bounds, Context, FocusHandle, Keystroke, Pixels, SharedString, Window};
use mixless_acquire::imports::{ImportReport, ImportService};
use mixless_acquire_yt::YoutubeMusicAcquire;
use mixless_protocol::{
    Command, CueKind, DeckId, EngineSnapshot, EqBand, FxParams, FxSlot, PlaylistId, TempoMap,
    Track, TrackId, Waveform,
};
use mixless_spotify::SpotifyClient;

pub struct AppCore {
    pub engine: mixless_engine::Engine,
    pub library: mixless_library::Library,
    pub analyzer: mixless_analyze::Analyzer,
    pub acquired_dir: PathBuf,
    pub midi: Mutex<Option<mixless_midi::MidiHub>>,
    /// Serializes background loads so a cancelled worker cannot overwrite a manual load.
    pub deck_load: Mutex<()>,
}

pub enum ImportMsg {
    Progress(String),
    Done(Result<ImportReport, String>),
}

pub fn app_core() -> Arc<AppCore> {
    // Same data dir as the previous Tauri build, so libraries carry over.
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("app.mixless.desktop");
    std::fs::create_dir_all(&data_dir).ok();
    let acquired_dir = data_dir.join("acquired");
    std::fs::create_dir_all(&acquired_dir).ok();

    let library =
        mixless_library::Library::open(&data_dir.join("library.db")).expect("open library");
    let engine =
        mixless_engine::Engine::new(mixless_engine::EngineConfig::default()).expect("engine");
    let midi = mixless_midi::MidiHub::start(data_dir.join("midi.json")).ok();
    Arc::new(AppCore {
        engine,
        library,
        analyzer: mixless_analyze::Analyzer::new(),
        acquired_dir,
        midi: Mutex::new(midi),
        deck_load: Mutex::new(()),
    })
}

#[derive(Clone, Copy, PartialEq)]
pub enum WaveLayout {
    Top,
    Center,
}

#[derive(Clone)]
pub struct FxUi {
    pub kind: &'static str,
    pub on: bool,
    pub mix: f32,
}

pub const FX_KINDS: [&str; 5] = ["Echo", "Flanger", "Gate", "Reverb", "Phaser"];

pub fn default_fx() -> [FxUi; 3] {
    [
        FxUi {
            kind: "Echo",
            on: false,
            mix: 0.5,
        },
        FxUi {
            kind: "Flanger",
            on: false,
            mix: 0.5,
        },
        FxUi {
            kind: "Gate",
            on: false,
            mix: 0.5,
        },
    ]
}

/// One drag in progress. Started by a control's mouse-down, tracked by the
/// root element's mouse-move/up handlers so drags survive leaving the control.
/// Faders and the crossfader map the pointer position absolutely inside the
/// captured lane bounds (jump-to-position, like a hardware fader); knobs, the
/// jog wheel and the waveform use relative motion.
pub enum DragCtl {
    Knob {
        ctl: KnobCtl,
        start_y: f32,
        start_val: f32,
    },
    Fader {
        ctl: FaderCtl,
        bounds: Bounds<Pixels>,
    },
    Xfader {
        bounds: Bounds<Pixels>,
    },
    /// Jog scrubbing: angular input around the platter center (circular
    /// gestures), linear vertical input near the center. One full revolution
    /// maps to `frames_per_rev` source frames so the pointer and the spinning
    /// marker stay in sync.
    Jog {
        deck: DeckId,
        cx: f32,
        cy: f32,
        /// The gesture mode is selected at mouse-down and remains stable for
        /// the whole drag. This avoids jumping between angular/linear input.
        angular: bool,
        last_angle: Option<f32>,
        last_y: f32,
        dead_radius: f32,
        frames_per_rev: f64,
    },
    /// Waveform scrubbing: pointer motion along the time axis dispatches the
    /// same Jog command the platter uses, so both stay in sync.
    Wave {
        deck: DeckId,
        vertical: bool,
        frames_per_px: f32,
        last_pos: f32,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum KnobCtl {
    Master,
    Key(DeckId),
    Gain(DeckId),
    Filter(DeckId),
    Resonance(DeckId),
    Eq(DeckId, EqBand),
    FxMix(DeckId, usize),
}

#[derive(Clone, Copy, Debug)]
pub enum FaderCtl {
    Tempo(DeckId),
    Channel(DeckId),
}

impl KnobCtl {
    pub fn range(self) -> (f32, f32, f32) {
        match self {
            KnobCtl::Master => (0.0, 1.0, 0.01),
            KnobCtl::Key(_) => (-6.0, 6.0, 0.1),
            KnobCtl::Gain(_) => (-12.0, 12.0, 0.1),
            KnobCtl::Filter(_) => (-1.0, 1.0, 0.01),
            KnobCtl::Resonance(_) => (0.0, 1.0, 0.01),
            KnobCtl::Eq(_, _) => (-12.0, 12.0, 0.1),
            KnobCtl::FxMix(_, _) => (0.0, 1.0, 0.01),
        }
    }
}

impl FaderCtl {
    pub fn range(self) -> (f32, f32, f32) {
        match self {
            FaderCtl::Tempo(_) => (0.88, 1.12, 0.001),
            FaderCtl::Channel(_) => (0.0, 1.0, 0.01),
        }
    }
}

pub struct UiState {
    pub core: Arc<AppCore>,
    pub snapshot: EngineSnapshot,
    pub tracks: Arc<Vec<Track>>,
    pub playlists: Arc<Vec<mixless_library::PlaylistSummary>>,
    pub playlist_sel: Option<i64>,
    pub track_sel: Option<i64>,
    pub focus: DeckId,
    pub url: String,
    pub url_focus: FocusHandle,
    pub show_import_modal: bool,
    pub error: SharedString,
    pub acquire: SharedString,
    pub import_details: Vec<String>,
    pub playlist_issues: Vec<mixless_library::ImportItem>,
    pub busy: bool,
    pub picker_open: bool,
    pub show_fx: bool,
    pub wave_layout: WaveLayout,
    pub fx: [[FxUi; 3]; 2],
    pub wave: [Option<(i64, Arc<Waveform>)>; 2],
    pub wave_tempo: [Option<Arc<TempoMap>>; 2],
    grid_rx: [Option<Receiver<Option<TempoMap>>>; 2],
    pub drag: Option<DragCtl>,
    /// `(deck, slot, was_on)` for the FX button currently held by the mouse.
    /// The root mouse-up capture restores `was_on` even when the pointer leaves
    /// the small HOLD hitbox.
    momentary_fx: Option<(DeckId, usize, bool)>,
    import_rx: Option<Receiver<ImportMsg>>,
    artwork_rx: Option<Receiver<Result<usize, String>>>,
    pub automix_active: bool,
    pub automix_status: String,
    automix_epoch: Arc<AtomicU64>,
    automix_rx: Option<Receiver<crate::automix::AutomixMsg>>,
}

impl UiState {
    pub fn new(core: Arc<AppCore>, cx: &mut Context<Self>) -> Self {
        let (artwork_tx, artwork_rx) = channel();
        let artwork_core = core.clone();
        std::thread::spawn(move || {
            let result = artwork_core
                .library
                .backfill_artwork()
                .map_err(|error| error.to_string());
            let _ = artwork_tx.send(result);
        });
        Self {
            core: core.clone(),
            snapshot: core.engine.snapshot(),
            tracks: Arc::new(Vec::new()),
            playlists: Arc::new(Vec::new()),
            playlist_sel: None,
            track_sel: None,
            focus: DeckId::A,
            url: String::new(),
            url_focus: cx.focus_handle(),
            show_import_modal: false,
            error: "".into(),
            acquire: "".into(),
            import_details: Vec::new(),
            playlist_issues: Vec::new(),
            busy: false,
            picker_open: false,
            show_fx: true,
            wave_layout: WaveLayout::Top,
            fx: [default_fx(), default_fx()],
            wave: [None, None],
            wave_tempo: [None, None],
            grid_rx: [None, None],
            drag: None,
            momentary_fx: None,
            import_rx: None,
            artwork_rx: Some(artwork_rx),
            automix_active: false,
            automix_status: String::new(),
            automix_epoch: Arc::new(AtomicU64::new(0)),
            automix_rx: None,
        }
    }

    pub fn deck(&self, id: DeckId) -> &mixless_protocol::DeckSnapshot {
        &self.snapshot.decks[id.index()]
    }

    /* ---- engine commands ------------------------------------------------ */

    pub fn dispatch(&mut self, cmd: Command) {
        if let Err(e) = self.core.engine.dispatch(cmd) {
            self.error = e.to_string().into();
        }
    }

    pub fn play_pause(&mut self, deck: DeckId) {
        self.dispatch(Command::PlayPause { deck });
    }

    pub fn sync(&mut self, deck: DeckId) {
        self.dispatch(Command::Sync {
            deck,
            keylock: self.deck(deck).keylock,
        });
    }

    pub fn jump_cue(&mut self, deck: DeckId, index: usize) {
        self.dispatch(Command::JumpCue {
            deck,
            index: index as u8,
        });
    }

    pub fn set_cue_now(&mut self, deck: DeckId, index: usize) {
        let frame = self.deck(deck).frame;
        self.core.engine.set_cue_frame(deck, index as u8, frame);
        if let Some(id) = self.deck(deck).track_id {
            let _ = self
                .core
                .library
                .set_cue(id, index as u8, frame, CueKind::Hot, true);
        }
    }

    pub fn set_loop(&mut self, deck: DeckId, bars: u16, on: bool) {
        self.dispatch(Command::SetLoop { deck, bars, on });
    }

    pub fn set_pfl(&mut self, deck: DeckId, on: bool) {
        self.dispatch(Command::SetPfl { deck, on });
    }

    pub fn set_kill(&mut self, deck: DeckId, band: EqBand, on: bool) {
        self.dispatch(Command::SetEqKill { deck, band, on });
    }

    pub fn cycle_fx(&mut self, deck: DeckId, slot: usize, dir: i32) {
        let kinds = FX_KINDS;
        let idx = kinds
            .iter()
            .position(|k| *k == self.fx[deck.index()][slot].kind)
            .unwrap_or(0);
        let next = kinds[(idx as i32 + dir).rem_euclid(kinds.len() as i32) as usize];
        self.fx[deck.index()][slot].kind = next;
        self.apply_fx(deck, slot);
    }

    pub fn toggle_fx(&mut self, deck: DeckId, slot: usize) {
        self.fx[deck.index()][slot].on = !self.fx[deck.index()][slot].on;
        let on = self.fx[deck.index()][slot].on;
        if on {
            self.apply_fx(deck, slot);
        }
        self.dispatch(Command::SetFxBypass {
            deck,
            slot: fx_slot(slot),
            on: !on,
        });
    }

    /// Apply an FX slot only while the pointer is held down. The previous
    /// latched state is restored on release, including releases outside the
    /// button handled by the root capture listener.
    pub fn begin_momentary_fx(&mut self, deck: DeckId, slot: usize) {
        if self.momentary_fx.is_some() {
            return;
        }
        let was_on = self.fx[deck.index()][slot].on;
        self.fx[deck.index()][slot].on = true;
        if !was_on {
            self.apply_fx(deck, slot);
        }
        self.dispatch(Command::SetFxBypass {
            deck,
            slot: fx_slot(slot),
            on: false,
        });
        self.momentary_fx = Some((deck, slot, was_on));
    }

    /// Stop a held FX and restore its persistent toggle. Returns whether a
    /// held FX was released so callers can request one repaint.
    pub fn end_momentary_fx(&mut self) -> bool {
        let Some((deck, slot, was_on)) = self.momentary_fx.take() else {
            return false;
        };
        if !was_on {
            self.fx[deck.index()][slot].on = false;
            self.dispatch(Command::SetFxBypass {
                deck,
                slot: fx_slot(slot),
                on: true,
            });
        }
        true
    }

    pub fn momentary_fx_active(&self, deck: DeckId, slot: usize) -> bool {
        self.momentary_fx
            .map(|(held_deck, held_slot, _)| held_deck == deck && held_slot == slot)
            .unwrap_or(false)
    }

    pub fn set_fx_mix(&mut self, deck: DeckId, slot: usize, mix: f32) {
        self.fx[deck.index()][slot].mix = mix;
        self.apply_fx(deck, slot);
    }

    pub fn reset_knob(&mut self, ctl: KnobCtl) {
        let value = match ctl {
            KnobCtl::Master => 0.8,
            KnobCtl::Key(_) => 0.0,
            KnobCtl::Gain(_) => 0.0,
            KnobCtl::Filter(_) => 0.0,
            KnobCtl::Resonance(_) => 0.35,
            KnobCtl::Eq(_, _) => 0.0,
            KnobCtl::FxMix(_, _) => 0.5,
        };
        self.set_knob_value(ctl, value);
    }

    pub fn reset_fader(&mut self, ctl: FaderCtl) {
        let value = match ctl {
            FaderCtl::Tempo(_) => 1.0,
            // The engine's neutral channel level is 0.8, matching its startup
            // snapshot and leaving a little headroom for live mixing.
            FaderCtl::Channel(_) => 0.8,
        };
        self.set_fader_value(ctl, value);
    }

    pub fn reset_xfader(&mut self) {
        self.dispatch(Command::SetCrossfader { value: 0.0 });
    }

    fn apply_fx(&mut self, deck: DeckId, slot: usize) {
        let fx = &self.fx[deck.index()][slot];
        let params = FxParams {
            mix: fx.mix,
            kind: Some(fx.kind.to_lowercase()),
            ..Default::default()
        };
        self.dispatch(Command::SetFx {
            deck: Some(deck),
            slot: fx_slot(slot),
            params,
        });
    }

    /* ---- drag dispatch --------------------------------------------------- */

    pub fn begin_knob(&mut self, ctl: KnobCtl, y: f32) {
        let val = self.knob_value(ctl);
        self.drag = Some(DragCtl::Knob {
            ctl,
            start_y: y,
            start_val: val,
        });
    }

    pub fn begin_fader(&mut self, ctl: FaderCtl, y: f32, bounds: Bounds<Pixels>) {
        let (min, max, _) = ctl.range();
        let t = 1.0
            - ((y - f32::from(bounds.origin.y)) / f32::from(bounds.size.height)).clamp(0.0, 1.0);
        self.set_fader_value(ctl, min + t * (max - min));
        self.drag = Some(DragCtl::Fader { ctl, bounds });
    }

    pub fn begin_xfader(&mut self, x: f32, bounds: Bounds<Pixels>) {
        let t = ((x - f32::from(bounds.origin.x)) / f32::from(bounds.size.width)).clamp(0.0, 1.0);
        let v = (t * 2.0 - 1.0).clamp(-1.0, 1.0);
        self.dispatch(Command::SetCrossfader { value: v });
        self.drag = Some(DragCtl::Xfader { bounds });
    }

    /// Begin a jog scrub. The platter bounds give the drag center; the
    /// revolution rate (33⅓ rpm → 1.8 s per revolution) makes one full
    /// circular gesture scrub exactly 1.8 s of audio, matching the marker.
    pub fn begin_jog(&mut self, deck: DeckId, x: f32, y: f32, bounds: Bounds<Pixels>) {
        self.end_drag();
        self.dispatch(Command::SetJogTouch {
            deck,
            touching: true,
        });
        let (cx, cy) = (
            f32::from(bounds.origin.x) + f32::from(bounds.size.width) / 2.0,
            f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0,
        );
        let d = self.deck(deck);
        let sr = if d.src_sample_rate > 0 {
            d.src_sample_rate
        } else {
            self.snapshot.sample_rate
        };
        let r = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
        let platter_radius =
            (f32::from(bounds.size.width).min(f32::from(bounds.size.height)) / 2.0 - 6.0).max(8.0);
        // Grabs on the label behave like a conventional vertical scratch;
        // grabs on the vinyl follow the pointer angle one-to-one.
        let dead_radius = (platter_radius * 0.30).max(18.0);
        let angular = r > dead_radius;
        let last_angle = angular.then(|| (y - cy).atan2(x - cx));
        self.drag = Some(DragCtl::Jog {
            deck,
            cx,
            cy,
            angular,
            last_angle,
            last_y: y,
            dead_radius,
            frames_per_rev: sr as f64 * 1.8,
        });
    }

    /// Begin a waveform scrub along its time axis.
    pub fn begin_wave(&mut self, deck: DeckId, vertical: bool, pos: f32, frames_per_px: f32) {
        self.end_drag();
        self.dispatch(Command::SetJogTouch {
            deck,
            touching: true,
        });
        self.drag = Some(DragCtl::Wave {
            deck,
            vertical,
            frames_per_px,
            last_pos: pos,
        });
    }

    pub fn drag_move(&mut self, x: f32, y: f32) {
        match self.drag.take() {
            Some(DragCtl::Knob {
                ctl,
                start_y,
                start_val,
            }) => {
                let (min, max, step) = ctl.range();
                let dv = ((start_y - y) / 120.0) * (max - min);
                let v = (start_val + dv).clamp(min, max);
                let v = (v / step).round() * step;
                self.set_knob_value(ctl, v.clamp(min, max));
                self.drag = Some(DragCtl::Knob {
                    ctl,
                    start_y,
                    start_val,
                });
            }
            Some(DragCtl::Fader { ctl, bounds }) => {
                let (min, max, _) = ctl.range();
                let t = 1.0
                    - ((y - f32::from(bounds.origin.y)) / f32::from(bounds.size.height))
                        .clamp(0.0, 1.0);
                self.set_fader_value(ctl, min + t * (max - min));
                self.drag = Some(DragCtl::Fader { ctl, bounds });
            }
            Some(DragCtl::Xfader { bounds }) => {
                let t = ((x - f32::from(bounds.origin.x)) / f32::from(bounds.size.width))
                    .clamp(0.0, 1.0);
                let v = (t * 2.0 - 1.0).clamp(-1.0, 1.0);
                self.dispatch(Command::SetCrossfader { value: v });
                self.drag = Some(DragCtl::Xfader { bounds });
            }
            Some(DragCtl::Jog {
                deck,
                cx,
                cy,
                angular,
                mut last_angle,
                mut last_y,
                dead_radius,
                frames_per_rev,
            }) => {
                // Keep the mode chosen at mouse-down. While an angular drag
                // crosses the center dead zone, re-anchor without seeking so
                // it cannot produce a discontinuity on the far side.
                let (dx, dy) = (x - cx, y - cy);
                let r = (dx * dx + dy * dy).sqrt();
                let frames = if angular && r > dead_radius {
                    let cur = dy.atan2(dx);
                    let mut d = match last_angle {
                        Some(prev) => cur - prev,
                        None => 0.0,
                    };
                    // Wrap to (-π, π]; clamp guards the discontinuity when
                    // the pointer sweeps across the center.
                    while d > std::f32::consts::PI {
                        d -= 2.0 * std::f32::consts::PI;
                    }
                    while d < -std::f32::consts::PI {
                        d += 2.0 * std::f32::consts::PI;
                    }
                    last_angle = Some(cur);
                    (d as f64 / (2.0 * std::f64::consts::PI)) * frames_per_rev
                } else if angular {
                    last_angle = None;
                    0.0
                } else {
                    (last_y - y) as f64 * 180.0 * frames_per_rev / (44_100.0 * 1.8)
                };
                if frames.abs() >= 0.01 {
                    self.dispatch(Command::Jog {
                        deck,
                        delta_frames: frames as f32,
                    });
                }
                last_y = y;
                self.drag = Some(DragCtl::Jog {
                    deck,
                    cx,
                    cy,
                    angular,
                    last_angle,
                    last_y,
                    dead_radius,
                    frames_per_rev,
                });
            }
            Some(DragCtl::Wave {
                deck,
                vertical,
                frames_per_px,
                mut last_pos,
            }) => {
                // Horizontal lanes scrub forward on rightward motion; vertical
                // lanes scrub forward on upward motion.
                let delta = if vertical { last_pos - y } else { x - last_pos };
                let frames = delta * frames_per_px;
                if frames.abs() >= 0.01 {
                    self.dispatch(Command::Jog {
                        deck,
                        delta_frames: frames,
                    });
                }
                last_pos = if vertical { y } else { x };
                self.drag = Some(DragCtl::Wave {
                    deck,
                    vertical,
                    frames_per_px,
                    last_pos,
                });
            }
            None => {}
        }
    }

    pub fn end_drag(&mut self) {
        if let Some(DragCtl::Jog { deck, .. } | DragCtl::Wave { deck, .. }) = self.drag.take() {
            self.dispatch(Command::SetJogTouch {
                deck,
                touching: false,
            });
        }
    }

    pub fn knob_value(&self, ctl: KnobCtl) -> f32 {
        match ctl {
            KnobCtl::Master => self.snapshot.master,
            KnobCtl::Key(d) => self.deck(d).pitch_semitones,
            KnobCtl::Gain(d) => self.deck(d).gain_db,
            KnobCtl::Filter(d) => self.deck(d).filter_amount,
            KnobCtl::Resonance(d) => self.deck(d).filter_resonance,
            KnobCtl::Eq(d, band) => self.deck(d).eq_db[band.index()],
            KnobCtl::FxMix(d, s) => self.fx[d.index()][s].mix,
        }
    }

    fn set_knob_value(&mut self, ctl: KnobCtl, v: f32) {
        match ctl {
            KnobCtl::Master => self.dispatch(Command::SetMaster { value: v }),
            KnobCtl::Key(d) => self.dispatch(Command::SetPitchSemitones {
                deck: d,
                semitones: v,
            }),
            KnobCtl::Gain(d) => self.dispatch(Command::SetChannelGain { deck: d, db: v }),
            KnobCtl::Filter(d) => self.dispatch(Command::SetChannelFilter { deck: d, amount: v }),
            KnobCtl::Resonance(d) => self.dispatch(Command::SetFilterResonance {
                deck: d,
                resonance: v,
            }),
            KnobCtl::Eq(d, band) => self.dispatch(Command::SetEq {
                deck: d,
                band,
                db: v,
            }),
            KnobCtl::FxMix(d, s) => self.set_fx_mix(d, s, v),
        }
    }

    fn set_fader_value(&mut self, ctl: FaderCtl, v: f32) {
        match ctl {
            FaderCtl::Tempo(d) => self.dispatch(Command::SetRate { deck: d, rate: v }),
            FaderCtl::Channel(d) => self.dispatch(Command::SetChannelFader { deck: d, value: v }),
        }
    }

    /* ---- text inputs ------------------------------------------------------ */

    /// Minimal single-line input handling: printable keys, backspace, paste,
    /// escape/enter to blur. No IME in v1.
    pub fn handle_url_input_key(
        &mut self,
        ks: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match ks.key.as_str() {
            "escape" => {
                self.show_import_modal = false;
                window.blur();
            }
            "enter" => {
                if !self.busy && !self.url.trim().is_empty() {
                    let url = self.url.trim().to_string();
                    self.import_spotify(url);
                }
                window.blur();
            }
            "backspace" => {
                self.url.pop();
            }
            _ => {
                if ks.modifiers.platform && ks.key == "v" {
                    if let Some(item) = cx.read_from_clipboard()
                        && let Some(text) = item.text()
                    {
                        let text = text.replace(['\n', '\r'], " ");
                        self.url.push_str(&text);
                    }
                } else if !ks.modifiers.control && !ks.modifiers.platform && !ks.modifiers.alt {
                    if let Some(ch) = &ks.key_char
                        && !ch.chars().any(|c| c.is_control())
                    {
                        self.url.push_str(ch);
                    }
                }
            }
        }
        cx.notify();
    }

    /* ---- library --------------------------------------------------------- */

    pub fn refresh_tracks(&mut self) {
        self.playlist_issues = self
            .playlist_sel
            .and_then(|id| self.core.library.import_items(PlaylistId(id)).ok())
            .unwrap_or_default()
            .into_iter()
            .filter(|item| !matches!(item.status.as_str(), "local" | "acquired"))
            .collect();
        match if let Some(id) = self.playlist_sel {
            self.core
                .library
                .playlist_tracks(PlaylistId(id))
                .map_err(|e| e.to_string())
        } else {
            self.core.library.list_tracks().map_err(|e| e.to_string())
        } {
            Ok(list) => self.tracks = Arc::new(list),
            Err(e) => self.error = e.to_string().into(),
        }
    }

    pub fn refresh_playlists(&mut self) {
        match self.core.library.list_playlists() {
            Ok(list) => self.playlists = Arc::new(list),
            Err(e) => self.error = e.to_string().into(),
        }
    }

    pub fn select_playlist(&mut self, id: Option<i64>) {
        self.playlist_sel = id;
        self.track_sel = None;
        self.refresh_tracks();
    }

    pub fn load_deck(&mut self, deck: DeckId, track_id: TrackId) {
        self.stop_automix();
        let core = self.core.clone();
        std::thread::spawn(move || {
            load_deck_blocking(&core, deck, track_id);
        });
    }

    pub fn toggle_automix(&mut self) {
        if self.automix_active {
            self.stop_automix();
            return;
        }
        let tracks: Vec<_> = self
            .playlist_sel
            .and_then(|id| {
                self.core
                    .library
                    .playlist_tracks(mixless_protocol::PlaylistId(id))
                    .ok()
            })
            .map(|tracks| tracks.into_iter().map(|track| track.id).collect())
            .unwrap_or_else(|| self.tracks.iter().map(|t| t.id).collect());
        if tracks.len() < 2 {
            self.error = "Select at least two local tracks in the library or a playlist".into();
            return;
        }
        let generation = self.automix_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.automix_active = true;
        self.automix_status = "Preparing transition…".into();
        self.error = "".into();
        let (tx, rx) = channel();
        self.automix_rx = Some(rx);
        let core = self.core.clone();
        let epoch = self.automix_epoch.clone();
        std::thread::spawn(move || crate::automix::run(core, tracks, epoch, generation, tx));
    }

    pub fn stop_automix(&mut self) {
        self.automix_epoch.fetch_add(1, Ordering::AcqRel);
        self.automix_active = false;
        self.automix_status.clear();
        self.automix_rx = None;
        self.dispatch(Command::StopAutomix);
    }

    /* ---- imports ---------------------------------------------------------- */

    pub fn choose_local(&mut self, folders: bool, cx: &mut Context<Self>) {
        if self.busy || self.picker_open {
            return;
        }
        self.picker_open = true;
        let selection = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: !folders,
            directories: folders,
            multiple: true,
            prompt: Some(
                if folders {
                    "Import folders"
                } else {
                    "Import audio files"
                }
                .into(),
            ),
        });
        cx.spawn(async move |this, cx| {
            let result = selection.await;
            let _ = this.update(cx, |s, cx| {
                s.picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        s.show_import_modal = false;
                        s.import_files(paths);
                    }
                    Ok(Err(error)) => s.error = error.to_string().into(),
                    Err(error) => s.error = error.to_string().into(),
                    _ => {}
                }
                cx.notify();
            });
        })
        .detach();
        cx.notify();
    }

    pub fn import_files(&mut self, paths: Vec<PathBuf>) {
        if self.busy || paths.is_empty() {
            return;
        }
        self.busy = true;
        self.error = "".into();
        self.acquire = "Preparing local import…".into();
        self.import_details.clear();
        let (tx, rx) = channel::<ImportMsg>();
        self.import_rx = Some(rx);
        let core = self.core.clone();
        std::thread::spawn(move || {
            import_files_blocking(&core, paths, &tx);
        });
    }

    pub fn import_spotify(&mut self, url: String) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.error = "".into();
        self.acquire = "Reading Spotify playlist…".into();
        self.import_details.clear();
        let (tx, rx) = channel::<ImportMsg>();
        self.import_rx = Some(rx);
        let core = self.core.clone();
        std::thread::spawn(move || {
            import_spotify_blocking(&core, &url, &tx);
        });
    }

    /* ---- poll ------------------------------------------------------------ */

    /// Pull engine snapshot, drain MIDI + import progress, refresh waveform
    /// caches. Returns whether any UI-visible state changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        if let Some(rx) = self.automix_rx.take() {
            let mut finished = false;
            while let Ok(message) = rx.try_recv() {
                changed = true;
                match message {
                    crate::automix::AutomixMsg::Status(status) => self.automix_status = status,
                    crate::automix::AutomixMsg::Done(result) => {
                        self.automix_active = false;
                        finished = true;
                        self.automix_status = "Playlist complete".into();
                        if let Err(error) = result {
                            self.error = error.into();
                            self.automix_status.clear();
                        }
                    }
                }
            }
            if !finished {
                self.automix_rx = Some(rx);
            }
        }
        if let Ok(midi) = self.core.midi.lock() {
            if let Some(hub) = midi.as_ref() {
                for cmd in hub.drain() {
                    let _ = self.core.engine.dispatch(cmd);
                }
            }
        }
        let snapshot = self.core.engine.snapshot();
        changed |= snapshot != self.snapshot;
        self.snapshot = snapshot;

        for (i, deck_id) in [DeckId::A, DeckId::B].into_iter().enumerate() {
            let tid = self.snapshot.decks[i].track_id.map(|t| t.0);
            let cached = self.wave[i].as_ref().map(|(id, _)| *id);
            if tid != cached {
                changed = true;
                self.wave_tempo[i] = None;
                self.grid_rx[i] = tid.map(|id| {
                    let (tx, rx) = channel();
                    let core = self.core.clone();
                    std::thread::spawn(move || {
                        let tempo = core
                            .library
                            .load_analysis(TrackId(id), mixless_analyze::ANALYSIS_VERSION)
                            .ok()
                            .flatten()
                            .map(|analysis| {
                                let mut tempo = analysis.tempo;
                                for positions in [&mut tempo.beats, &mut tempo.downbeats] {
                                    positions.retain(|value| value.is_finite() && *value >= 0.0);
                                    positions.sort_by(f32::total_cmp);
                                    positions.dedup();
                                }
                                tempo
                            });
                        let _ = tx.send(tempo);
                    });
                    rx
                });
                self.wave[i] = self
                    .core
                    .engine
                    .deck_waveform(deck_id)
                    .map(|w| (tid.unwrap_or(-1), Arc::new(w)));
            }
        }

        for i in 0..2 {
            if let Some(rx) = self.grid_rx[i].take() {
                match rx.try_recv() {
                    Ok(tempo) => {
                        self.wave_tempo[i] = tempo.map(Arc::new);
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => self.grid_rx[i] = Some(rx),
                    Err(_) => {}
                }
            }
        }
        if let Some(rx) = self.import_rx.take() {
            let mut finished = false;
            loop {
                match rx.try_recv() {
                    Ok(ImportMsg::Progress(p)) => {
                        self.acquire = p.into();
                        changed = true;
                    }
                    Ok(ImportMsg::Done(Ok(report))) => {
                        self.acquire = report.summary().into();
                        self.import_details =
                            report.warning.into_iter().chain(report.errors).collect();
                        self.playlist_sel = report.playlist.map(|id| id.0);
                        self.busy = false;
                        finished = true;
                        changed = true;
                    }
                    Ok(ImportMsg::Done(Err(e))) => {
                        self.error = e.into();
                        self.busy = false;
                        finished = true;
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        if !finished {
                            self.error = "Import worker stopped unexpectedly; completed tracks are retained. Retry the import.".into();
                            self.busy = false;
                            finished = true;
                            changed = true;
                        }
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                }
            }
            if finished {
                self.refresh_tracks();
                self.refresh_playlists();
            } else {
                self.import_rx = Some(rx);
            }
        }
        if let Some(rx) = self.artwork_rx.take() {
            match rx.try_recv() {
                Ok(Ok(updated)) => {
                    if updated > 0 {
                        self.refresh_tracks();
                        changed = true;
                    }
                }
                Ok(Err(error)) => tracing::warn!("artwork backfill: {error}"),
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.artwork_rx = Some(rx);
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
            }
        }
        changed
    }

    /// State that must track the display's VSync rather than a wall-clock
    /// timer. Idle windows fall back to the lightweight background poller.
    pub fn needs_continuous_repaint(&self) -> bool {
        self.drag.is_some()
            || self
                .snapshot
                .decks
                .iter()
                .any(|deck| deck.playing || deck.level.iter().any(|level| *level > 0.0003))
    }
}

pub fn fx_slot(i: usize) -> FxSlot {
    match i {
        0 => FxSlot::Insert0,
        1 => FxSlot::Insert1,
        _ => FxSlot::Insert2,
    }
}

/// Load a track onto a deck on a background thread: decode + waveform are slow.
pub fn load_deck_blocking(core: &AppCore, deck: DeckId, track_id: TrackId) {
    let _load = core.deck_load.lock().expect("deck load mutex");
    let track = match core.library.get_track(track_id) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("load_deck: {e}");
            return;
        }
    };
    let cues = core.library.cues(track.id).unwrap_or_default();
    if let Err(e) = core.engine.load_file(
        deck,
        track.id,
        std::path::Path::new(&track.path),
        track.title.clone(),
        track.artist.clone(),
    ) {
        tracing::warn!("load_deck decode: {e}");
        return;
    }
    if let Some(bpm) = track.bpm {
        core.engine.set_bpm(deck, bpm);
    }
    for cue in cues {
        core.engine.set_cue_frame(deck, cue.index, cue.frame);
    }
}

fn import_files_blocking(core: &AppCore, paths: Vec<PathBuf>, tx: &Sender<ImportMsg>) {
    let service = ImportService {
        library: &core.library,
        analyzer: &core.analyzer,
    };
    let _ = tx.send(ImportMsg::Progress("Scanning folders…".into()));
    let (paths, errors) = mixless_acquire::local_paths::collect_audio(&paths);
    if paths.is_empty() {
        let message = if errors.is_empty() {
            "No supported audio files found in this selection.".into()
        } else {
            errors.join("\n")
        };
        let _ = tx.send(ImportMsg::Done(Err(message)));
        return;
    }
    let mut report = service.local_files(&paths, |message| {
        let _ = tx.send(ImportMsg::Progress(message));
    });
    report.errors.extend(errors);
    if !report.errors.is_empty() {
        report.warning =
            Some("Some files or folders could not be imported; open import details.".into());
    }
    let _ = tx.send(ImportMsg::Done(Ok(report)));
}

fn import_spotify_blocking(core: &AppCore, url: &str, tx: &Sender<ImportMsg>) {
    let result = (|| -> Result<ImportReport, String> {
        let playlist = SpotifyClient::new()
            .fetch_playlist(url)
            .map_err(|e| e.to_string())?;
        let service = ImportService {
            library: &core.library,
            analyzer: &core.analyzer,
        };
        let mut provider = None;
        service.spotify_playlist(
            &playlist,
            |job| {
                // Local-only imports work even if no downloader is installed.
                if provider.is_none() {
                    provider = Some(YoutubeMusicAcquire::detect()?);
                }
                provider.as_ref().unwrap().fetch(job, &core.acquired_dir)
            },
            |message| {
                let _ = tx.send(ImportMsg::Progress(message));
            },
        )
    })();
    let _ = tx.send(ImportMsg::Done(result));
}
