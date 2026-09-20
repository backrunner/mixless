---
title: "Your first mix"
description: "Install Mixless, bring in your music, and start a continuous set."
order: 1
image: /images/social.png
imageAlt: "Mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

Mixless is a native DJ workspace for macOS. You can mix by hand, let AutoMix handle the transitions, or move between the two.

## Install

Get an installer from [GitHub Releases](https://github.com/backrunner/mixless/releases). Open the DMG and move **Mixless** into **Applications**. The app targets **macOS 12 or later** on Apple Silicon and Intel. Releases marked **Pre-release** belong to the beta channel.

To build from source, follow the [development guide](/docs/development).

## Bring in a few tracks

Choose **+ Files** or **+ Folder** in the library. Folders include subfolders; importing the same path again does not duplicate the track. Each folder gets its own playlist.

Rows appear while importing, before analysis is finished. Waveforms, tempo, key, and musical structure are prepared in the background. Native stem and note analysis downloads verified models on first use; your audio is processed on your Mac.

## Check your output

Open **Mixless → Preferences…** (or **Settings…**) with **Cmd+,**. In **Audio I/O**, choose your master output and apply it. Start with a comfortable listening level. A separate headphone output is optional; see [audio setup](/docs/audio).

## Start the set

1. Select the playlist you want to play.
2. Drag rows to choose the order.
3. Press **AUTO**. With empty decks, Mixless loads and starts the first playable track.
4. Use sequence or shuffle for the following tracks. AutoMix repeats the playlist.

Or drag a track onto deck A or B and press Play to mix manually. Audio becomes playable after decoding and waveform preparation; beat and key analysis can arrive afterward.

## Make it your own

Set hot cues, choose IN and OUT cue roles, adjust EQ, or move a fader while AutoMix runs. A manual edit takes control of that automation lane. Loading a track manually takes over from AutoMix.

Continue with [AutoMix](/docs/automix), [decks and effects](/docs/decks), or the [keyboard reference](/docs/shortcuts).

## Updates

Release builds check their own stable or beta channel at launch. You can also choose **Mixless → Check for Updates…**. After an update is installed, restart when the app asks. Development builds skip automatic updates.
