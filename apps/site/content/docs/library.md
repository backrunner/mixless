---
title: "Your music library"
description: "Import folders, arrange playlists, and load your decks."
order: 2
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

## Files and folders

Use **+ Files** for individual tracks or **+ Folder** for a recursive folder import. Imports run in the background and can be queued. Duplicate paths are skipped.

Under **LOCAL FOLDERS**, the selected root includes its subfolders. Immediate parent folders remain separately selectable. Hover a folder label to see its complete path. Reimporting adds new files without clearing existing members.

**All Tracks** is the combined view. Startup and completed imports select a playlist; the combined view remains available explicitly.

## Arrange a playlist

Drag anywhere on a track row to reorder it. The insertion line marks the destination; hold near the top or bottom of the list to scroll. Drop below the last row to move a track to the end, or press **Esc** to cancel.

Orders are saved independently for each folder, playlist, and All Tracks. Right-click a playlist to duplicate or remove it. Removal asks for confirmation. Removing a folder playlist hides it until that folder is imported again.

## Load a deck

Drag a library track onto either deck. The destination highlights during the drag; loaded rows show A/B badges. Both decks can load independently. Loading a track manually takes control from AutoMix.

The library shows full-track waveforms and numbered cue flags. During AutoMix it also shows planned IN/OUT points and mixing intervals. See [cue points](/docs/decks).

## Spotify playlists

The **Spotify** entry imports playlist metadata and shows import progress. Spotify does **not** provide decoded audio for mixing. Tracks must match local files or be obtained through a configured acquisition provider before playback.

Optional acquisition uses separately installed tools such as `yt-dlp` and `FFmpeg`; availability depends on your setup and the selected service. Use only audio you have the right to use. Acquisition failures do not turn metadata-only rows into playable tracks.

For imported Spotify tracks, missing covers can be fetched from Spotify and cached locally. Embedded artwork from local audio is also supported. See [privacy](/privacy) for network activity.

## Analysis and caching

Playable audio and deeper analysis are prepared separately. Tempo, key, waveforms, and stem data are cached across restarts; outdated or changed files are analyzed again. Playback remains available during model preparation or a model failure.

Manage derived files in [Storage](/docs/storage).

## Missing audio files

If a source file has moved, mixless searches known track folders, imported folders and its download directory in the background. Matching file contents reconnect automatically, preserving the track, playlists and cues. The search is bounded; files outside known folders may need to be selected manually.

If the file still cannot be located, the row shows **File not found**. Hover the label to see the saved path. **Locate file…** selects the corresponding local audio file and resumes preparation; **Retry** searches again after a folder or disk becomes available. **Remove** removes the track from the library and all playlists without deleting audio. These actions are also available in the track's right-click menu.

A manually selected file is checked before the saved path changes. If its contents differ, mixless refreshes analysis and keeps manually placed cues. Other analysis failures remain labeled **Analysis failed**; hover that label for the error.

## Portable packages

Open **+** in the library and use **Portable package**. **Entire library** exports every track and playlist; turn it off to select several playlists. You can also right-click a playlist and choose **Export package…**.

- **Export…** creates a `.mixpack` file containing original audio, metadata, playlist order, cues, artwork, and existing analysis, waveforms, and stems. Shared audio blocks are stored once.
- **Update existing…** updates the selected playlists in the same file and keeps its other playlists. Entire-library updates replace that source library's package snapshot. Only new or changed blocks are appended; interrupted updates retain the previous complete commit.
- **Import…** restores the package into managed local storage. After import, the package and the original audio folders can be disconnected. Reimporting updates the same source records without duplicating them; other local music is kept.

Imported stems play without model downloads or separation, including on Intel Macs. Export includes caches that have already finished; it does not create missing stems. The result shows how many tracks lack analysis or stems. Finish preparation on the source machine before exporting if every track needs them. Use matching app versions to keep analysis compatible; incompatible stem caches are rejected.

Folder playlists become ordinary playlists on the receiving machine. Unavailable Spotify entries remain unavailable metadata; packages do not download their audio.

Package operations run in the background and can be cancelled. Incremental updates retain old data inside the file; export to a new file to reclaim space. Import needs enough local space for audio and stems.
