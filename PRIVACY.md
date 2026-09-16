# Privacy

Mixless is a local-first desktop application. It contains **no telemetry,
analytics, tracking, or crash reporting** of any kind.

## What stays on your device

- The music library, analysis results (beat grids, keys, waveforms, stem and
  note caches), cue points and playlist order live in a local `library.db` in
  the application support directory.
- Preferences and MIDI maps persist in `preferences.json` and `midi.json`
  beside that database.
- Imported and acquired audio stays on your disk. Its use remains subject to
  the rights you hold in it.

## What leaves your device, and only when you ask

- **Spotify sign-in and playlists.** If you choose to connect Spotify, the app
  talks to the Spotify Web API for OAuth and playlist metadata. Credentials
  and tokens are stored locally; Mixless never receives audio from Spotify and
  sends nothing about your local library back.
- **Model downloads.** The first time stem/note analysis runs, the app
  downloads pinned ONNX model artifacts over HTTPS from Hugging Face
  (`StemSplitio/htdemucs-onnx`) and GitHub (`spotify/basic-pitch`). No audio or
  analysis results are uploaded; inference is fully on-device.
- **Optional acquisition.** When you explicitly acquire a playlist track,
  separately installed `yt-dlp`/`FFmpeg` may contact the services you select
  (e.g. YouTube Music). Those requests are made by the external tools under
  their own terms; Mixless does not proxy or inspect them.

## Data deletion

Derived caches (stems, analysis, waveforms, artwork, downloaded models) are
never evicted automatically; the Storage tab in Preferences reports their size
and can clear them per category or per playlist. Source audio, playlists and
cue points are kept. Removing the application support directory (library
database, artwork, stem cache, models, preferences) removes all data Mixless
stores. Uninstalling the app does not automatically delete that directory.
