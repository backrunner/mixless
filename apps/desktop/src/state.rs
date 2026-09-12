//! Shared app core + UI state. The engine/library live behind `Arc` so
//! background threads can import and load tracks without blocking the UI.

mod artwork;
mod cues;
mod transport;
pub use transport::TransportButton;
mod library_refresh;

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

use crate::fader::FaderLane;

use gpui::{AppContext, Bounds, Context, FocusHandle, Keystroke, Pixels, SharedString, Window};
use mixless_acquire::imports::{ImportReport, ImportService};
use mixless_acquire_yt::YoutubeMusicAcquire;
use mixless_protocol::{
    Command, CueKind, DeckId, EngineSnapshot, EqBand, FxKind, FxSlot, FxState, PlaylistId,
    TempoMap, Track, TrackId, Waveform,
};
use mixless_spotify::SpotifyClient;

pub struct AppCore {
    pub analysis: crate::analysis::AnalysisJobs,
    pub mix_preparation: crate::automix::Preparation,
    pub automix_commit: Mutex<()>,
    pub settings: crate::settings::SettingsStore,
    pub midi_error: Mutex<Option<String>>,
    pub settings_apply: Mutex<()>,
    pub engine: mixless_engine::Engine,
    pub library: mixless_library::Library,
    pub analyzer: mixless_analyze::Analyzer,
    pub acquired_dir: PathBuf,
    pub midi: Mutex<Option<Arc<mixless_midi::MidiHub>>>,
    /// Serializes background loads so a cancelled worker cannot overwrite a manual load.
    pub deck_load: Mutex<()>,
}

pub enum ImportMsg {
    Progress(String),
    LibraryChanged,
    Done(Result<ImportReport, String>),
}

enum ImportRequest {
    Local(Vec<PathBuf>),
    Spotify(String),
}

pub fn app_core() -> Arc<AppCore> {
    // Same data dir as the previous Tauri build, so libraries carry over.
    let data_dir = std::env::var_os("MIXLESS_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            dirs::data_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("app.mixless.desktop")
        });
    std::fs::create_dir_all(&data_dir).ok();
    let acquired_dir = data_dir.join("acquired");
    std::fs::create_dir_all(&acquired_dir).ok();

    let library =
        mixless_library::Library::open(&data_dir.join("library.db")).expect("open library");
    let settings = crate::settings::SettingsStore::load(data_dir.join("preferences.json"));
    let initial = settings.get();
    let engine = mixless_engine::Engine::new_with_audio(
        mixless_engine::EngineConfig {
            offline: std::env::var_os("MIXLESS_OFFLINE").is_some(),
            ..Default::default()
        },
        initial.audio.clone(),
    )
    .expect("engine");
    initial.apply_playback(&engine);
    // Open the desktop mixer with both channel caps at their bottom zero mark.
    // Initialize the engine too, so the first snapshot and actual level agree.
    for deck in [DeckId::A, DeckId::B] {
        engine
            .dispatch(Command::SetChannelFader {
                deck,
                value: FaderCtl::Channel(deck).reset_value(),
            })
            .expect("initialize channel level");
    }
    let midi = mixless_midi::MidiHub::start_with_config(data_dir.join("midi.json"), initial.midi);
    let midi_error = midi.as_ref().err().map(ToString::to_string);
    Arc::new(AppCore {
        analysis: crate::analysis::AnalysisJobs::default(),
        mix_preparation: Default::default(),
        automix_commit: Mutex::new(()),
        engine,
        library,
        analyzer: mixless_analyze::Analyzer::new(),
        acquired_dir,
        settings,
        midi_error: Mutex::new(midi_error),
        settings_apply: Mutex::new(()),
        midi: Mutex::new(midi.ok().map(Arc::new)),
        deck_load: Mutex::new(()),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaveLayout {
    Top,
    Center,
}

/// One drag in progress. Started by a control's mouse-down, tracked by the
/// root element's mouse-move/up handlers so drags survive leaving the control.
/// Vertical faders preserve the grab point on the cap; clicking the lane jumps
/// to that position. The crossfader maps absolutely inside its captured bounds;
/// knobs, the jog wheel and the waveform use relative motion.
pub enum DragCtl {
    Knob {
        ctl: KnobCtl,
        start_y: f32,
        start_val: f32,
    },
    Fader {
        ctl: FaderCtl,
        lane: FaderLane,
        grab_offset: f32,
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
    FxParam(DeckId, usize, crate::fx::FxParam),
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
            KnobCtl::FxMix(_, _) | KnobCtl::FxParam(_, _, _) => (0.0, 1.0, 0.01),
        }
    }
}

impl FaderCtl {
    pub fn reset_value(self) -> f32 {
        match self {
            Self::Tempo(_) => 1.0,
            Self::Channel(_) => 0.0,
        }
    }

    pub fn range(self) -> (f32, f32, f32) {
        match self {
            FaderCtl::Tempo(_) => (0.88, 1.12, 0.001),
            FaderCtl::Channel(_) => (0.0, 1.0, 0.01),
        }
    }
}

pub struct UiState {
    pub library_view: gpui::Entity<crate::views::library::LibraryView>,
    pub library_previews: crate::wave::preview::PreviewStore,
    pub library_key: Option<crate::views::library::LibraryKey>,
    library_refresh: library_refresh::LibraryRefresh,
    settings_revision: u64,
    pub analysis_revision: u64,
    pub core: Arc<AppCore>,
    pub snapshot: EngineSnapshot,
    pub tracks: Arc<Vec<Track>>,
    pub playlists: Arc<Vec<mixless_library::PlaylistSummary>>,
    pub playlist_sel: Option<i64>,
    pub track_sel: Option<i64>,
    pub focus: DeckId,
    pub url: String,
    pub url_focus: FocusHandle,
    pub keyboard_focus: FocusHandle,
    pub show_shortcuts: bool,
    pub show_import_modal: bool,
    pub error: SharedString,
    pub acquire: SharedString,
    pub import_details: Vec<String>,
    pub playlist_issues: Vec<mixless_library::ImportItem>,
    pub busy: bool,
    pub picker_open: bool,
    pub show_fx: bool,
    pub wave_layout: WaveLayout,
    pub fx: [[FxState; FxSlot::INSERTS.len()]; 2],
    pub fx_editor: Option<(DeckId, usize)>,
    pub wave: [Option<(i64, Arc<Waveform>)>; 2],
    pub wave_cache: [Option<Arc<crate::wave::WaveCache>>; 2],
    pub presentation_frames: [f64; 2],
    pub deck_artwork: [artwork::DeckArtwork; 2],
    artwork_cache: Arc<artwork::ArtworkCache>,
    wave_cache_rx: [Option<Receiver<Arc<crate::wave::WaveCache>>>; 2],
    pub wave_tempo: [Option<Arc<TempoMap>>; 2],
    grid_rx: [Option<Receiver<Result<TempoMap, String>>>; 2],
    deck_load_rx: [Option<Receiver<Result<(), String>>>; 2],
    deck_load_epoch: Arc<[AtomicU64; 2]>,
    sync_request: Option<crate::beat_sync::SyncRequest>,
    pub cue_shift: [bool; 2],
    pub cue_role_editor: [Option<(usize, TrackId)>; 2],
    transport_press: Option<transport::TransportPress>,
    pub drag: Option<DragCtl>,
    pub pending_drag: Option<(f32, f32)>,
    /// `(deck, slot, was_on)` for the FX button currently held by the mouse.
    /// The root mouse-up capture restores `was_on` even when the pointer leaves
    /// the small HOLD hitbox.
    momentary_fx: Option<(DeckId, usize, bool)>,
    import_rx: Option<Receiver<ImportMsg>>,
    import_queue: std::collections::VecDeque<ImportRequest>,
    artwork_rx: Option<Receiver<Result<usize, String>>>,
    pub automix_active: bool,
    pub automix_shuffle: Arc<std::sync::atomic::AtomicBool>,
    pub automix_status: String,
    pub automix_plan: Option<(DeckId, Arc<mixless_protocol::MixPlan>)>,
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
        let owner = cx.entity().downgrade();
        let library_view = cx.new(|_| crate::views::library::LibraryView { owner });
        Self {
            library_view,
            library_key: None,
            library_previews: Default::default(),
            library_refresh: library_refresh::LibraryRefresh::default(),
            settings_revision: 0,
            analysis_revision: 0,
            core: core.clone(),
            snapshot: core.engine.snapshot(),
            tracks: Arc::new(Vec::new()),
            playlists: Arc::new(Vec::new()),
            playlist_sel: None,
            track_sel: None,
            focus: DeckId::A,
            url: String::new(),
            url_focus: cx.focus_handle(),
            keyboard_focus: cx.focus_handle(),
            show_shortcuts: false,
            show_import_modal: false,
            error: "".into(),
            acquire: "".into(),
            import_details: Vec::new(),
            playlist_issues: Vec::new(),
            busy: false,
            picker_open: false,
            show_fx: true,
            wave_layout: WaveLayout::Top,
            fx: [
                mixless_protocol::default_fx(),
                mixless_protocol::default_fx(),
            ],
            fx_editor: None,
            wave: [None, None],
            wave_cache: [None, None],
            presentation_frames: [0.; 2],
            deck_artwork: Default::default(),
            artwork_cache: Default::default(),
            wave_cache_rx: [None, None],
            wave_tempo: [None, None],
            grid_rx: [None, None],
            deck_load_rx: [None, None],
            deck_load_epoch: Arc::new(std::array::from_fn(|_| AtomicU64::new(0))),
            sync_request: None,
            cue_shift: [false; 2],
            cue_role_editor: [None; 2],
            transport_press: None,
            drag: None,
            pending_drag: None,
            momentary_fx: None,
            import_rx: None,
            import_queue: Default::default(),
            artwork_rx: Some(artwork_rx),
            automix_active: false,
            automix_shuffle: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            automix_status: String::new(),
            automix_plan: None,
            automix_epoch: Arc::new(AtomicU64::new(0)),
            automix_rx: None,
        }
    }

    pub fn deck(&self, id: DeckId) -> &mixless_protocol::DeckSnapshot {
        &self.snapshot.decks[id.index()]
    }

    /* ---- engine commands ------------------------------------------------ */

    pub fn dispatch(&mut self, cmd: Command) {
        if self
            .sync_request
            .as_ref()
            .is_some_and(|r| r.cancelled_by(&cmd))
        {
            self.sync_request = None;
        }
        if let Err(e) = self.core.engine.dispatch(cmd) {
            self.error = e.to_string().into();
        }
    }

    pub fn play_pause(&mut self, deck: DeckId) {
        if self.core.engine.snapshot().deck(deck).frames == 0 {
            return;
        }
        if self.automix_active {
            self.stop_automix();
        }
        self.dispatch(Command::PlayPause { deck });
    }

    pub fn grid_pending(&self, deck: DeckId) -> bool {
        self.grid_rx[deck.index()].is_some()
    }

    pub fn sync(&mut self, deck: DeckId) {
        let snapshot = self.core.engine.snapshot();
        let d = snapshot.deck(deck);
        self.error = "".into();
        if self.sync_waiting(deck) {
            self.sync_request = None;
            return;
        }
        self.sync_request = None;
        if d.synced {
            self.dispatch(Command::DisableSync { deck });
            return;
        }
        // Cancel the host planner as well as its active audio plan, so a
        // background transition cannot take timing back after manual Sync.
        if let Some(request) = crate::beat_sync::SyncRequest::new(deck, &snapshot) {
            self.stop_automix();
            self.sync_request = Some(request);
        } else {
            self.error = "Load two tracks and turn off reverse before syncing".into();
        }
    }

    pub fn sync_waiting(&self, deck: DeckId) -> bool {
        self.sync_request
            .as_ref()
            .is_some_and(|r| r.follower == deck)
    }

    pub fn jump_cue(&mut self, deck: DeckId, index: usize) {
        if self.automix_active {
            self.stop_automix();
        }
        self.dispatch(Command::JumpCue {
            deck,
            index: index as u8,
        });
    }

    /// Empty pads set a cue; populated pads jump without changing playback.
    pub fn trigger_cue(&mut self, deck: DeckId, index: usize, shift: bool) {
        let snapshot = self.core.engine.snapshot();
        let d = &snapshot.decks[deck.index()];
        if index >= d.cues.len() || d.track_id.is_none() {
            return;
        }
        if shift || self.cue_shift[deck.index()] {
            if d.cues[index].is_none() {
                self.set_cue_now(deck, index);
            }
            self.cue_role_editor[deck.index()] = Some((index, d.track_id.unwrap()));
            self.cue_shift[deck.index()] = false;
        } else if d.cues[index].is_none() {
            self.set_cue_now(deck, index);
        } else {
            self.jump_cue(deck, index);
        }
    }

    pub fn set_loop(&mut self, deck: DeckId, beats: f32, on: bool) {
        self.dispatch(Command::SetLoopBeats { deck, beats, on });
    }

    pub fn set_pfl(&mut self, deck: DeckId, on: bool) {
        self.dispatch(Command::SetPfl { deck, on });
    }

    pub fn set_kill(&mut self, deck: DeckId, band: EqBand, on: bool) {
        self.dispatch(Command::SetEqKill { deck, band, on });
    }

    pub fn cycle_fx(&mut self, deck: DeckId, slot: usize, dir: i32) {
        let kinds = FxKind::ALL;
        let idx = kinds
            .iter()
            .position(|k| *k == self.fx[deck.index()][slot].kind)
            .unwrap_or(0);
        let next = kinds[(idx as i32 + dir).rem_euclid(kinds.len() as i32) as usize];
        self.select_fx(deck, slot, next);
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
            KnobCtl::FxParam(deck, slot, param) => {
                param.value(&FxState::new(self.fx[deck.index()][slot].kind))
            }
        };
        self.set_knob_value(ctl, value);
    }

    pub fn reset_fader(&mut self, ctl: FaderCtl) {
        self.end_drag();
        self.set_fader_value(ctl, ctl.reset_value());
    }

    pub fn reset_xfader(&mut self) {
        self.dispatch(Command::SetCrossfader { value: 0.0 });
    }

    pub(crate) fn apply_fx(&mut self, deck: DeckId, slot: usize) {
        let fx = &self.fx[deck.index()][slot];
        let params = fx.params();
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

    pub fn begin_fader(&mut self, ctl: FaderCtl, y: f32, bounds: Bounds<Pixels>, displayed: f32) {
        let lane = FaderLane::new(bounds.origin.y.into(), bounds.size.height.into());
        let grab = lane.grab_offset(y, displayed);
        if grab.is_none() {
            let (min, max, _) = ctl.range();
            self.set_fader_value(ctl, min + lane.value(y) * (max - min));
        }
        self.drag = Some(DragCtl::Fader {
            ctl,
            lane,
            grab_offset: grab.unwrap_or(0.0),
        });
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
        if self.deck(deck).frames == 0 {
            return;
        }
        self.focus = deck;
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
        if self.deck(deck).frames == 0 {
            return;
        }
        self.focus = deck;
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
            Some(DragCtl::Fader {
                ctl,
                lane,
                grab_offset,
            }) => {
                let (min, max, _) = ctl.range();
                let t = lane.value(y - grab_offset);
                self.set_fader_value(ctl, min + t * (max - min));
                self.drag = Some(DragCtl::Fader {
                    ctl,
                    lane,
                    grab_offset,
                });
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
                // Grab the waveform: moving content right/down moves source
                // time backward under the fixed playhead in either layout.
                let delta = last_pos - if vertical { y } else { x };
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
        self.pending_drag = None;
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
            KnobCtl::FxParam(d, s, param) => param.value(&self.fx[d.index()][s]),
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
            KnobCtl::FxParam(d, s, param) => {
                param.set(&mut self.fx[d.index()][s], v);
                self.apply_fx(d, s);
            }
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
                if !self.url.trim().is_empty() {
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

    pub fn select_playlist(&mut self, id: Option<i64>) {
        self.library_refresh.initial = false;
        self.playlist_sel = id;
        self.track_sel = None;
        self.tracks = Arc::new(Vec::new());
        self.refresh_tracks();
    }

    pub fn load_deck(&mut self, deck: DeckId, track_id: TrackId) {
        self.sync_request = None;
        self.stop_automix();
        let index = deck.index();
        let generation = self.deck_load_epoch[index].fetch_add(1, Ordering::AcqRel) + 1;
        let epoch = self.deck_load_epoch.clone();
        let core = self.core.clone();
        let (tx, rx) = channel();
        let (grid_tx, grid_rx) = channel();
        self.deck_load_rx[index] = Some(rx);
        self.grid_rx[index] = Some(grid_rx);
        self.error = "".into();
        std::thread::spawn(move || {
            let active = || epoch[index].load(Ordering::Acquire) == generation;
            let loaded_hash = match crate::analysis::load_manual(&core, deck, track_id, &active) {
                Ok(Some(hash)) => {
                    let _ = tx.send(Ok(()));
                    hash
                }
                Ok(None) => return,
                Err(error) => {
                    let _ = tx.send(Err(error));
                    return;
                }
            };
            let grid = (|| {
                let prepared = crate::analysis::prepare(&core, track_id)?;
                if prepared.track.content_hash != loaded_hash {
                    return Err("File changed after loading; reload the track".into());
                }
                let _commit = core
                    .automix_commit
                    .lock()
                    .map_err(|_| "Deck load lock poisoned")?;
                if !active() {
                    return Err("Load replaced".into());
                }
                if prepared.analysis.tempo.beats.len() >= 2 {
                    core.engine
                        .set_beat_grid(deck, track_id, prepared.analysis.tempo.clone())
                        .map_err(|e| e.to_string())?;
                }
                core.engine
                    .set_bpm(deck, prepared.analysis.tempo.global_bpm);
                for cue in core.library.cues(track_id).map_err(|e| e.to_string())? {
                    core.engine.set_cue_frame(deck, cue.index, cue.frame);
                    let _ = core.engine.dispatch(mixless_protocol::Command::SetCueKind {
                        track_id,
                        index: cue.index,
                        kind: if cue.user_set {
                            cue.kind
                        } else {
                            mixless_protocol::CueKind::Hot
                        },
                    });
                }
                core.analysis.loaded.lock().expect("loaded analysis")[index] =
                    Some(prepared.clone());
                Ok(prepared.analysis.tempo.clone())
            })();
            if active() {
                let _ = grid_tx.send(grid);
            }
        });
    }

    pub fn toggle_automix(&mut self) {
        self.sync_request = None;
        if self.automix_active {
            self.stop_automix();
            return;
        }
        let tracks: Vec<_> = self.tracks.iter().map(|track| track.id).collect();
        if tracks.is_empty() {
            self.error = "Import a track to start Automix".into();
            return;
        }
        for epoch in self.deck_load_epoch.iter() {
            epoch.fetch_add(1, Ordering::AcqRel);
        }
        self.deck_load_rx = [None, None];
        self.grid_rx = [None, None];
        let generation = self.automix_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.automix_active = true;
        self.automix_status = "Loading next track".into();
        self.error = "".into();
        let (tx, rx) = channel();
        self.automix_rx = Some(rx);
        let core = self.core.clone();
        let epoch = self.automix_epoch.clone();
        let shuffle = self.automix_shuffle.clone();
        std::thread::spawn(move || {
            crate::automix::run(core, tracks, shuffle, epoch, generation, tx)
        });
    }

    pub fn stop_automix(&mut self) {
        let core = self.core.clone();
        let _commit = core.automix_commit.lock().expect("automix commit");
        self.automix_epoch.fetch_add(1, Ordering::AcqRel);
        self.automix_active = false;
        self.automix_status.clear();
        self.automix_plan = None;
        self.automix_rx = None;
        self.dispatch(Command::StopAutomix);
    }

    /* ---- imports ---------------------------------------------------------- */

    pub fn choose_local(&mut self, folders: bool, cx: &mut Context<Self>) {
        if self.picker_open {
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
        if paths.is_empty() {
            return;
        }
        // Publish selected directories immediately, including queued and empty ones.
        let mut first = None;
        for path in &paths {
            if path.is_dir() {
                match self.core.library.register_folder(path) {
                    Ok(id) => {
                        first.get_or_insert(id);
                    }
                    Err(error) => self.error = error.to_string().into(),
                }
            }
        }
        self.refresh_playlists();
        if let Some(id) = first {
            self.select_playlist(Some(id.0));
        }
        self.import_queue.push_back(ImportRequest::Local(paths));
        self.start_next_import();
    }

    pub fn import_spotify(&mut self, url: String) {
        self.import_queue.push_back(ImportRequest::Spotify(url));
        self.start_next_import();
    }

    pub fn import_status(&self) -> SharedString {
        if self.import_queue.is_empty() {
            return self.acquire.clone();
        }
        format!(
            "{} · {} imports queued",
            self.acquire,
            self.import_queue.len()
        )
        .into()
    }

    fn start_next_import(&mut self) {
        if self.import_rx.is_some() {
            return;
        }
        let Some(request) = self.import_queue.pop_front() else {
            self.busy = false;
            return;
        };
        self.busy = true;
        self.error = "".into();
        self.acquire = match &request {
            ImportRequest::Local(_) => "Finding audio files",
            ImportRequest::Spotify(_) => "Reading playlist",
        }
        .into();
        let (tx, rx) = channel();
        self.import_rx = Some(rx);
        let core = self.core.clone();
        std::thread::spawn(move || match request {
            ImportRequest::Local(paths) => import_files_blocking(&core, paths, &tx),
            ImportRequest::Spotify(url) => import_spotify_blocking(&core, &url, &tx),
        });
    }

    pub fn poll(&mut self) -> bool {
        let mut changed = self.poll_transport_press();
        changed |= self.poll_library();
        if self.library_previews.poll() {
            self.library_key = None;
            changed = true;
        }
        let analysis_revision = self.core.analysis.revision.load(Ordering::Acquire);
        if self.analysis_revision != analysis_revision {
            self.analysis_revision = analysis_revision;
            let updates = self.core.analysis.take_updates();
            for id in updates.keys() {
                self.library_previews.invalidate(*id);
            }
            if self
                .tracks
                .iter()
                .any(|track| updates.contains_key(&track.id))
            {
                for track in Arc::make_mut(&mut self.tracks) {
                    if let Some(updated) = updates.get(&track.id) {
                        *track = updated.clone();
                    }
                }
            }
            if !self.busy {
                self.acquire = self.core.analysis.summary().into();
            }
            changed = true;
        }
        let revision = self.core.settings.revision();
        if self.settings_revision != revision {
            let settings = self.core.settings.get();
            self.wave_layout = settings.wave_layout;
            self.show_fx = settings.show_fx;
            self.settings_revision = revision;
            changed = true;
        }
        if let Some(rx) = self.automix_rx.take() {
            let mut finished = false;
            while let Ok(message) = rx.try_recv() {
                changed = true;
                match message {
                    crate::automix::AutomixMsg::Status(status) => self.automix_status = status,
                    crate::automix::AutomixMsg::Preparing(title) => {
                        self.automix_plan = None;
                        self.automix_status = format!("Loading {title}");
                    }
                    crate::automix::AutomixMsg::Plan(deck, plan) => {
                        self.automix_plan = Some((deck, plan))
                    }
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
        let midi_commands = self
            .core
            .midi
            .lock()
            .ok()
            .and_then(|midi| midi.as_ref().map(|hub| hub.drain()))
            .unwrap_or_default();
        for cmd in midi_commands {
            match cmd {
                Command::Sync { deck, .. } => self.sync(deck),
                Command::JumpCue { deck, index } => self.trigger_cue(deck, index as usize, false),
                cmd => self.dispatch(cmd),
            }
        }
        let snapshot = self.core.engine.snapshot();
        changed |= snapshot != self.snapshot;
        self.fx = std::array::from_fn(|index| snapshot.decks[index].fx);
        self.snapshot = snapshot;
        self.presentation_frames = self.core.engine.presentation_frames(&self.snapshot);

        for index in 0..2 {
            if let Some(rx) = self.deck_load_rx[index].take() {
                match rx.try_recv() {
                    Ok(Ok(())) => changed = true,
                    Ok(Err(error)) => {
                        self.error = error.into();
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        self.deck_load_rx[index] = Some(rx)
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.error = "Deck load worker stopped unexpectedly".into();
                        changed = true;
                    }
                }
            }
        }
        for (i, deck_id) in [DeckId::A, DeckId::B].into_iter().enumerate() {
            let tid = self.snapshot.decks[i].track_id.map(|t| t.0);
            let cached = self.wave[i].as_ref().map(|(id, _)| *id);
            let current_wave = self.core.engine.deck_waveform(deck_id);
            let wave_changed = match (&self.wave[i], &current_wave) {
                (Some((_, old)), Some(current)) => !Arc::ptr_eq(old, current),
                (None, None) => false,
                _ => true,
            };
            changed |= self.deck_artwork[i].poll(
                &self.core,
                &self.artwork_cache,
                self.snapshot.decks[i].track_id,
                wave_changed,
            );
            if tid != cached || wave_changed {
                changed = true;
                self.wave_tempo[i] = None;
                self.wave[i] = current_wave.map(|w| (tid.unwrap_or(-1), w));
                self.wave_cache[i] = None;
                self.wave_cache_rx[i] = None;
                if let Some((_, wave)) = &self.wave[i] {
                    let (tx, rx) = channel();
                    let wave = wave.clone();
                    self.wave_cache_rx[i] = Some(rx);
                    std::thread::spawn(move || {
                        let _ = tx.send(Arc::new(crate::wave::WaveCache::new(wave)));
                    });
                }
            }
            if let Some(rx) = self.wave_cache_rx[i].take() {
                match rx.try_recv() {
                    Ok(cache) => {
                        if self.wave[i]
                            .as_ref()
                            .is_some_and(|(_, wave)| Arc::ptr_eq(wave, &cache.source))
                        {
                            self.wave_cache[i] = Some(cache);
                            changed = true;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => self.wave_cache_rx[i] = Some(rx),
                    Err(_) => {}
                }
            }
            if tid.is_some() && self.wave_tempo[i].is_none() {
                if let Ok(loaded) = self.core.analysis.loaded.try_lock() {
                    if let Some(prepared) = &loaded[i] {
                        if Some(prepared.track.id.0) == tid {
                            self.wave_tempo[i] = Some(Arc::new(prepared.analysis.tempo.clone()));
                            changed = true;
                        }
                    }
                }
            }
        }

        for i in 0..2 {
            if let Some(rx) = self.grid_rx[i].take() {
                match rx.try_recv() {
                    Ok(result) => {
                        match result {
                            Ok(tempo) => self.wave_tempo[i] = Some(Arc::new(tempo)),
                            Err(error) => {
                                self.error =
                                    format!("Deck {}: {error}", if i == 0 { "A" } else { "B" })
                                        .into()
                            }
                        }
                        changed = true;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => self.grid_rx[i] = Some(rx),
                    Err(_) => {}
                }
            }
        }
        if let Some(request) = self.sync_request.take() {
            use crate::beat_sync::Progress;
            let snapshot = self.core.engine.snapshot();
            match request.progress(
                &snapshot,
                std::array::from_fn(|i| self.grid_rx[i].is_some()),
            ) {
                Progress::Waiting => self.sync_request = Some(request),
                Progress::Ready(command) => {
                    self.dispatch(command);
                    changed = true;
                }
                Progress::Cancelled => changed = true,
                Progress::Unavailable => {
                    if self.error.is_empty() {
                        self.error =
                            "Beat Sync unavailable: no usable beat grid on one or both tracks"
                                .into();
                    }
                    changed = true;
                }
            }
        }
        if let Some(rx) = self.import_rx.take() {
            let mut finished = false;
            loop {
                match rx.try_recv() {
                    Ok(ImportMsg::LibraryChanged) => {
                        self.refresh_tracks();
                        changed = true;
                    }
                    Ok(ImportMsg::Progress(p)) => {
                        self.acquire = p.into();
                        changed = true;
                    }
                    Ok(ImportMsg::Done(Ok(report))) => {
                        self.acquire = report.summary().into();
                        self.import_details =
                            report.warning.into_iter().chain(report.errors).collect();

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
                            self.error = "Import stopped before all files were added.".into();
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
                self.start_next_import();
            } else {
                self.import_rx = Some(rx);
            }
        }
        if let Some(rx) = self.artwork_rx.take() {
            match rx.try_recv() {
                Ok(Ok(updated)) => {
                    if updated > 0 {
                        for artwork in &mut self.deck_artwork {
                            artwork.invalidate();
                        }
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
            || self.transport_press.is_some()
            || self
                .snapshot
                .decks
                .iter()
                .any(|deck| deck.playing || deck.level.iter().any(|level| *level > 0.0003))
    }
}

pub fn fx_slot(i: usize) -> FxSlot {
    FxSlot::INSERTS[i]
}

fn import_files_blocking(core: &Arc<AppCore>, paths: Vec<PathBuf>, tx: &Sender<ImportMsg>) {
    let roots: Vec<_> = paths
        .iter()
        .filter(|p| p.is_dir())
        .filter_map(|p| p.canonicalize().ok())
        .collect();
    let mut report = ImportReport::default();
    let mut batch = Vec::new();
    let mut ids = Vec::new();
    let mut publish = |batch: &mut Vec<PathBuf>| {
        if batch.is_empty() {
            return;
        }
        report.total += batch.len();
        match core.library.register_local_files_into(batch, &roots) {
            Ok(added) => ids.extend(added),
            Err(_) => {
                // A file removed during scanning must not hide its healthy siblings.
                for path in batch.iter() {
                    match core
                        .library
                        .register_local_files_into(std::slice::from_ref(path), &roots)
                    {
                        Ok(added) => ids.extend(added),
                        Err(error) => {
                            report.failed += 1;
                            report.errors.push(format!("{}: {error}", path.display()));
                        }
                    }
                }
            }
        }
        batch.clear();
        let _ = tx.send(ImportMsg::LibraryChanged);
        let _ = tx.send(ImportMsg::Progress(format!(
            "Scanning folder · {} files",
            report.total
        )));
    };
    let mut last_publish = std::time::Instant::now();
    let errors = mixless_acquire::local_paths::visit_audio(&paths, |path| {
        batch.push(path);
        if batch.len() >= 64 || last_publish.elapsed() >= std::time::Duration::from_millis(50) {
            publish(&mut batch);
            last_publish = std::time::Instant::now();
        }
    });
    publish(&mut batch);
    report.errors.extend(errors);
    report.local = ids.len();
    for id in ids {
        core.analysis.forget(id);
    }
    match core.library.list_tracks() {
        Ok(tracks) => crate::analysis::schedule(core, &tracks),
        Err(error) => report.errors.push(error.to_string()),
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
