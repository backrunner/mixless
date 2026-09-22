//! Shared app core + UI state. The engine/library live behind `Arc` so
//! background threads can import and load tracks without blocking the UI.

mod artwork;
mod artwork_fetch;
mod audio;
mod automix;
mod controls;
mod cues;
mod import_retries;
mod imports;
mod loading;
mod midi;
mod playback;
mod poll;
mod transport;
mod url_input;
pub use transport::TransportButton;
mod library_actions;
mod library_order;
mod library_recovery;
mod library_refresh;
mod update;

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
    pub stems: Option<mixless_stems::Processor>,
    pub shutting_down: std::sync::atomic::AtomicBool,
    pub acquired_dir: PathBuf,
    pub midi: Mutex<Option<Arc<mixless_midi::MidiHub>>>,
    /// Serializes background loads so a cancelled worker cannot overwrite a manual load.
    pub deck_load: Mutex<()>,
    /// Set when a newer build's library had to be set aside; shown once at launch.
    pub library_notice: Option<String>,
    /// Self-update worker status; `poll_update` turns it into a banner.
    pub update: crate::update::Shared,
}

impl AppCore {
    /// Download root; Spotify imports use a playlist-named child folder.
    /// Falls back to the managed `acquired` directory inside the app data dir.
    pub fn download_dir(&self) -> PathBuf {
        self.settings
            .get()
            .download_dir
            .clone()
            .unwrap_or_else(|| self.acquired_dir.clone())
    }
}

/// A download folder must accept new files; probe before saving the choice.
pub fn ensure_writable_dir(path: &std::path::Path) -> Result<(), String> {
    let probe = path.join(".mixless-write-test");
    std::fs::create_dir_all(path)
        .and_then(|()| std::fs::File::create(&probe).map(|_| ()))
        .and_then(|()| std::fs::remove_file(&probe))
        .map_err(|e| format!("Folder is not writable: {e}"))
}

/// Compact a folder path for display by contracting the home directory to `~`.
pub fn display_dir(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

pub enum ImportMsg {
    Progress(String),
    LibraryChanged,
    PlaylistReady(PlaylistId),
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

    let db_path = data_dir.join("library.db");
    let (library, library_notice) = match mixless_library::Library::open(&db_path) {
        Ok(library) => (library, None),
        Err(error @ mixless_library::LibraryError::NewerSchema { .. }) => {
            // A downgrade met a schema this build cannot read. Set the file
            // aside and open a fresh library instead of crash-looping;
            // re-adding folders re-imports the tracks.
            tracing::warn!(%error, "library schema is newer; setting it aside");
            let backup = library_recovery::quarantine(&db_path)
                .unwrap_or_else(|e| panic!("cannot preserve incompatible library: {e}"));
            let notice = format!(
                "Your library was written by a newer version of mixless. A fresh library was opened; \
                 the previous database and its journals are preserved in {}. Reinstall the newer \
                 version and restore these files to recover it.",
                display_dir(&backup)
            );
            let library = mixless_library::Library::open(&db_path).expect("open library");
            (library, Some(notice))
        }
        Err(error) => panic!("open library: {error}"),
    };
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
        stems: mixless_stems::inference_supported().then(|| {
            mixless_stems::Processor::new(
                std::env::var_os("MIXLESS_MODEL_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| data_dir.join("models")),
                data_dir.join("stem-cache"),
                std::env::var_os("MIXLESS_MODELS_OFFLINE").is_none(),
            )
            .with_bundled_models(bundled_models())
        }),
        shutting_down: false.into(),
        acquired_dir,
        settings,
        midi_error: Mutex::new(midi_error),
        settings_apply: Mutex::new(()),
        midi: Mutex::new(midi.ok().map(Arc::new)),
        deck_load: Mutex::new(()),
        library_notice,
        update: Mutex::new(None),
    })
}

fn bundled_models() -> Option<PathBuf> {
    // Explicit development/test model directories remain fully isolated.
    if std::env::var_os("MIXLESS_MODEL_DIR").is_some() {
        return None;
    }
    #[cfg(target_os = "macos")]
    {
        let dir = crate::self_install::current_bundle()?.join("Contents/Resources/models");
        dir.is_dir().then_some(dir)
    }
    #[cfg(not(target_os = "macos"))]
    None
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
    Stem(DeckId, mixless_protocol::StemKind),
    Master,
    MasterGain,
    Key(DeckId),
    Trim(DeckId),
    Gain(DeckId),
    Balance(DeckId),
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
            KnobCtl::Stem(_, _) => (0., 1., 0.01),
            KnobCtl::Master => (0.0, 1.0, 0.01),
            KnobCtl::MasterGain => (-12.0, 12.0, 0.1),
            KnobCtl::Key(_) => (-6.0, 6.0, 0.1),
            KnobCtl::Trim(_) | KnobCtl::Gain(_) => (-12.0, 12.0, 0.1),
            KnobCtl::Balance(_) => (-1.0, 1.0, 0.01),
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
    library_order: library_order::LibraryOrder,
    pub track_scroll: gpui::UniformListScrollHandle,
    pub playlist_scroll: gpui::UniformListScrollHandle,
    midi_track_cursor: Option<(usize, TrackId)>,
    pub track_scroll_active: std::rc::Rc<std::cell::Cell<bool>>,
    settings_revision: u64,
    pub analysis_revision: u64,
    pub core: Arc<AppCore>,
    pub snapshot: EngineSnapshot,
    pub tracks: Arc<Vec<Track>>,
    pub playlists: Arc<Vec<mixless_library::PlaylistSummary>>,
    pub playlist_sel: Option<i64>,
    pub track_sel: Option<i64>,
    pub track_menu: Option<crate::views::library::TrackMenu>,
    pub playlist_menu: Option<crate::views::library::PlaylistMenu>,
    /// `(playlist id, display name)` awaiting removal confirmation.
    pub confirm_remove_playlist: Option<(i64, String)>,
    library_actions: Vec<Receiver<Result<TrackId, String>>>,
    playlist_actions: Vec<Receiver<Result<library_actions::PlaylistAction, String>>>,
    pub audio: audio::AudioUi,
    pub focus: DeckId,
    pub url: String,
    pub url_selection: url_input::UrlSelection,
    pub url_focus: FocusHandle,
    pub keyboard_focus: FocusHandle,
    pub show_shortcuts: bool,
    pub show_import_modal: bool,
    pub error: SharedString,
    pub update_notice: SharedString,
    update_entry: Option<crate::update::Entry>,
    update_notice_hold: bool,
    update_notice_at: Option<std::time::Instant>,
    pub acquire: SharedString,
    pub import_details: Vec<String>,
    pub playlist_imports: Vec<mixless_library::ImportItem>,
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
    pub deck_loading: [Option<String>; 2],
    /// Play pressed while the load worker owns the deck; drained on commit.
    pub pending_play: [bool; 2],
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
    import_retries: import_retries::Retries,
    artwork_fetch: artwork_fetch::ArtworkFetch,
    pub automix_active: bool,
    pub automix_shuffle: Arc<std::sync::atomic::AtomicBool>,
    pub automix_status: String,
    automix_preview_revision: u64,
    pub automix_plan: Option<(DeckId, Arc<mixless_protocol::MixPlan>)>,
    automix_epoch: Arc<AtomicU64>,
    automix_rx: Option<Receiver<crate::automix::AutomixMsg>>,
    automix_retry: Option<std::time::Instant>,
}

impl UiState {
    pub fn new(core: Arc<AppCore>, cx: &mut Context<Self>) -> Self {
        cx.on_app_quit(|state, _| {
            state.core.shutting_down.store(true, Ordering::Release);
            state.flush_library_order();
            async {}
        })
        .detach();
        let artwork_fetch = artwork_fetch::ArtworkFetch::start(core.clone());
        let owner = cx.entity().downgrade();
        let library_view = cx.new(|_| crate::views::library::LibraryView { owner });
        Self {
            library_view,
            library_key: None,
            library_order: library_order::LibraryOrder::default(),
            track_scroll: gpui::UniformListScrollHandle::new(),
            playlist_scroll: gpui::UniformListScrollHandle::new(),
            midi_track_cursor: None,
            track_scroll_active: Default::default(),
            library_previews: Default::default(),
            library_refresh: library_refresh::LibraryRefresh::default(),
            settings_revision: 0,
            analysis_revision: 0,
            core: core.clone(),
            snapshot: core.engine.snapshot(),
            tracks: Arc::new(Vec::new()),
            playlists: Arc::new(Vec::new()),
            playlist_sel: core.settings.get().last_playlist,
            track_sel: None,
            track_menu: None,
            playlist_menu: None,
            confirm_remove_playlist: None,
            library_actions: Vec::new(),
            playlist_actions: Vec::new(),
            audio: Default::default(),
            focus: DeckId::A,
            url: String::new(),
            url_selection: Default::default(),
            url_focus: cx.focus_handle(),
            keyboard_focus: cx.focus_handle(),
            show_shortcuts: false,
            show_import_modal: false,
            error: core.library_notice.clone().unwrap_or_default().into(),
            update_notice: "".into(),
            update_entry: None,
            update_notice_hold: false,
            update_notice_at: None,
            acquire: "".into(),
            import_details: Vec::new(),
            playlist_imports: Vec::new(),
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
            deck_loading: [None, None],
            pending_play: [false; 2],
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
            import_retries: Default::default(),
            artwork_fetch,
            automix_active: false,
            automix_shuffle: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            automix_status: String::new(),
            automix_preview_revision: 0,
            automix_plan: None,
            automix_epoch: Arc::new(AtomicU64::new(0)),
            automix_rx: None,
            automix_retry: None,
        }
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
