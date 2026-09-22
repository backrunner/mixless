---
title: "Privacy"
description: "Your music is processed on your Mac. Here is what the app stores and when it connects."
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

## On your device

mixless contains no telemetry, analytics, tracking, or crash reporting. Your library, analysis, waveforms, stems, cue points, playlists, preferences, and MIDI maps are stored locally. Audio and analysis are not uploaded for inference.

## Network activity

- **Models:** the standard installer downloads pinned, verified model files from Hugging Face and GitHub on first stem/note analysis. The installer with models reads the same files from the app without downloading them. Inference runs on device.
- **Spotify:** importing a playlist requests metadata. Missing artwork for linked Spotify tracks can be fetched in the background from Spotify and its image CDN. Spotify does not supply decoded audio for mixing.
- **Optional acquisition:** configured, separately installed tools such as yt-dlp and FFmpeg contact the services selected for acquiring audio. Those services have their own terms and privacy practices.
- **Updates:** release builds check GitHub for updates at launch and when requested. An available update downloads its manifest and installer; it does not upload your music library.

Normal HTTPS requests expose connection information, such as your IP address, to the contacted service. Local playback does not require these services once your files and models are prepared.

## Data removal

**Preferences → Storage** shows local usage and lets you clear regenerable caches. Clearing caches does not delete source audio, cue points, or playlists. Removing the application support directory deletes the data stored there, so back up anything you want to keep first. Uninstalling the app does not automatically remove that directory.

## This website

This site has no analytics scripts or advertising trackers. Documentation search runs locally in your browser. The hosting provider receives normal page requests. Links to GitHub and other services open sites with their own privacy practices.

For help, visit [support](/support).
