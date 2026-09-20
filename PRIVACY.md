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

## Network activity

- **Spotify playlists and artwork.** Importing a playlist requests metadata
  from Spotify. Missing covers for linked tracks may be fetched in the
  background from Spotify and its image CDN, then cached locally. Mixless never
  receives decoded mixing audio from Spotify or uploads local audio for this.
- **Model downloads.** The first time stem/note analysis runs, the app
  downloads pinned ONNX model artifacts over HTTPS from Hugging Face
  (`StemSplitio/htdemucs-onnx`) and GitHub (`spotify/basic-pitch`). No audio or
  analysis results are uploaded; inference is fully on-device.
- **Optional acquisition.** When you explicitly acquire a playlist track,
  separately installed `yt-dlp`/`FFmpeg` may contact the services you select
  (e.g. YouTube Music). Those requests are made by the external tools under
  their own terms; Mixless does not proxy or inspect them.

- **Updates.** Release builds contact GitHub at launch and on a manual update
  check. Available updates download a manifest and installer. These requests do
  not upload the music library.

These HTTPS requests expose normal connection information, such as your IP
address, to the contacted services. Once audio and models are prepared, local
playback and inference do not require a cloud service.

## Data deletion

Derived caches (stems, analysis, waveforms, artwork, downloaded models) are
never evicted automatically; the Storage tab in Preferences reports their size
and can clear them per category or per playlist. Source audio, playlists and
cue points are kept. Removing the application support directory (library
database, artwork, stem cache, models, preferences) removes all data Mixless
stores. Uninstalling the app does not automatically delete that directory.
