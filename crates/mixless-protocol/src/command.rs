use serde::{Deserialize, Serialize};

use crate::{CueKind, DeckId, EqBand, FilterKind, FxParams, FxSlot, PlaylistId, TrackId, XfCurve};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    LoadDeck {
        deck: DeckId,
        track_id: TrackId,
    },
    Eject {
        deck: DeckId,
    },
    PlayPause {
        deck: DeckId,
    },
    Jog {
        deck: DeckId,
        delta_frames: f32,
    },
    SetJogTouch {
        deck: DeckId,
        touching: bool,
    },
    SetPitchSemitones {
        deck: DeckId,
        semitones: f32,
    },
    SetRate {
        deck: DeckId,
        rate: f32,
    },
    SetKeyLock {
        deck: DeckId,
        on: bool,
    },
    Sync {
        deck: DeckId,
        keylock: bool,
    },
    /// Leave beat sync while preserving the current tempo.
    DisableSync {
        deck: DeckId,
    },
    /// Create a deck-local temporary cue, or jump to it without changing playback.
    TriggerTemporaryCue {
        deck: DeckId,
    },
    ClearTemporaryCue {
        deck: DeckId,
    },
    /// Audition only through the monitor bus, independently of channel faders.
    BeginCuePreview {
        deck: DeckId,
        frame: u64,
    },
    EndCuePreview {
        deck: DeckId,
    },
    JumpCue {
        deck: DeckId,
        index: u8,
    },
    SetCue {
        deck: DeckId,
        index: u8,
        frame: u64,
    },
    SetCueKind {
        track_id: TrackId,
        index: u8,
        kind: CueKind,
    },
    ClearCue {
        deck: DeckId,
        index: u8,
    },
    AutoCues {
        track_id: TrackId,
    },
    SetLoop {
        deck: DeckId,
        bars: u16,
        on: bool,
    },
    /// Beat units, including 1/16, 1/8, 1/4 and 1/2 beat.
    SetLoopBeats {
        deck: DeckId,
        beats: f32,
        on: bool,
    },
    LoopHalve {
        deck: DeckId,
    },
    LoopDouble {
        deck: DeckId,
    },
    SetEq {
        deck: DeckId,
        band: EqBand,
        db: f32,
    },
    SetEqKill {
        deck: DeckId,
        band: EqBand,
        on: bool,
    },
    SetFilter {
        deck: DeckId,
        cutoff_hz: f32,
        kind: FilterKind,
    },
    SetChannelFilter {
        deck: DeckId,
        amount: f32,
    },
    SetFilterResonanceEnabled {
        deck: DeckId,
        on: bool,
    },
    SetFilterResonance {
        deck: DeckId,
        resonance: f32,
    },
    SetChannelFader {
        deck: DeckId,
        value: f32,
    },
    /// Mixer TRIM in dB, independent of the deck limiter's input gain.
    SetChannelGain {
        deck: DeckId,
        db: f32,
    },
    /// Deck GAIN: -12 ..= 12 dB into the per-deck limiter, before the fader.
    SetDeckLimiterGain {
        deck: DeckId,
        db: f32,
    },
    /// Stereo balance, -1.0 (full left) ..= 1.0 (full right), 0.0 = center.
    SetBalance {
        deck: DeckId,
        value: f32,
    },
    SetStemGain {
        deck: DeckId,
        stem: crate::StemKind,
        value: f32,
    },
    SetCrossfader {
        value: f32,
    },
    SetXfCurve {
        curve: XfCurve,
    },
    SetXfReverse {
        on: bool,
    },
    SetMaster {
        value: f32,
    },
    /// Master input gain in dB, before the master limiter and output level.
    SetMasterGain {
        db: f32,
    },
    SetPfl {
        deck: DeckId,
        on: bool,
    },
    SetCueGain {
        value: f32,
    },
    SetCueDevice {
        name: Option<String>,
    },
    SetFx {
        deck: Option<DeckId>,
        slot: FxSlot,
        params: FxParams,
    },
    SetFxBypass {
        deck: DeckId,
        slot: FxSlot,
        on: bool,
    },
    SetFxSend {
        deck: DeckId,
        value: f32,
    },
    SetVinylMode {
        deck: DeckId,
        vinyl: bool,
        slip: bool,
    },
    /// Musical wet/dry fades; disabling retains a short click-prevention ramp.
    SetFxAutoFade {
        on: bool,
    },
    SetQuantize {
        on: bool,
    },
    BeatJump {
        deck: DeckId,
        bars: i16,
    },
    SetRoll {
        deck: DeckId,
        division: u16,
        on: bool,
    },
    SetReverse {
        deck: DeckId,
        on: bool,
    },
    /// Hold starts a continuous vinyl slowdown. Releasing an active brake
    /// stops playback; releasing an inactive brake does nothing.
    SetBrake {
        deck: DeckId,
        on: bool,
    },
    StartAutomix {
        playlist_id: PlaylistId,
        shuffle: bool,
    },
    PauseAutomix,
    ResumeAutomix,
    SkipAutomix,
    StopAutomix,
    CreatePlaylist {
        name: String,
    },
    RenamePlaylist {
        id: PlaylistId,
        name: String,
    },
    DeletePlaylist {
        id: PlaylistId,
    },
    ReorderPlaylist {
        id: PlaylistId,
        from: u32,
        to: u32,
    },
    AddToPlaylist {
        id: PlaylistId,
        track_id: TrackId,
        at: u32,
    },
    RemoveFromPlaylist {
        id: PlaylistId,
        position: u32,
    },
    ImportFiles {
        paths: Vec<String>,
    },
    RetryAnalysis {
        track_id: TrackId,
    },
    LinkLocalFile {
        spotify_track_id: String,
        path: String,
    },
    SpotifyLogin,
    SpotifyLogout,
}
