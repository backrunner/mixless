---
title: "Storage & offline use"
description: "Understand your local library, models, and regenerable caches."
order: 8
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

## What is stored locally

Your library database stores track metadata, analysis, cue points, and playlist order. Preferences and MIDI mappings live beside it. Audio, artwork, stem data, and model artifacts remain on your disk.

mixless does not upload audio for inference. The standard installer downloads verified HTDemucs and Basic Pitch ONNX models on first use. The [installer with models](https://mixless.alkinum.com/download?variant=bundled) includes those same models and loads them directly from the app, without a model download or a second cached copy. Stem and note inference runs locally on Apple Silicon without Python or a cloud model API; Intel Macs do not support this inference.

## Stem storage format

In **Preferences → General → Library → Stem storage**, choose how newly generated stems are stored:

| Choice | Format | Precision |
| --- | --- | --- |
| No compression (default) | 32-bit float WAV | Preserves the separator output. |
| Lossless | 24-bit FLAC, compression level 8 | Lossless encoding of 24-bit PCM. Converting the 32-bit float output to 24-bit PCM introduces a small rounding error. |
| Lossy | 320 kbps MP3 | Smaller files with lossy audio. |

All three retain stereo at 44.1 kHz. Peaks above full scale are scaled before compression and restored when loading to avoid clipping. MP3 delay and padding are removed on decode so every stem keeps the same sample positions and length.

Files are decoded to aligned float audio before they reach the playback engine. Cue points, loops, tempo, pitch and FX use that shared audio path. At unity stem gains, playback uses the original track. Lossy stems can leave audible artifacts when isolated or subtracted; WAV is the default for preserving the separated audio. Note and musical-structure analysis uses the original separated floats before compression.

Changing the setting keeps existing caches and does not interrupt playback. To regenerate them in the new format, clear the relevant stem cache in **Preferences → Storage** and run analysis again. Library packages preserve each cache's format; compressed stems require a version of mixless with compressed-cache support on the receiving Mac. Imported stems can also play on Intel Macs without running separation.

## Clear derived data

Open **Preferences → Storage** to inspect stem, analysis, waveform, artwork, and model caches, along with database and downloaded audio usage. Clear caches by category or scope supported cleanup to a playlist.

Caches are not evicted automatically. Clearing derived caches does not remove source audio, playlists, or cue points. Analysis or downloaded models may need to be prepared again afterward. The model cache entry counts downloaded copies only; clearing it leaves models included in the app intact.

## Prepare for offline use

For a new Mac that will stay offline, transfer the installer with models and your local audio files to it. On Apple Silicon, you can then install, import music and run new stem analyses without downloading models. With the standard installer, download models before disconnecting. Both variants use the same library, and automatic updates preserve the installed variant. Local playback and prepared analysis do not need a cloud service. Spotify metadata, missing artwork, optional audio acquisition, and update checks require their respective network services.

## Remove application data

Uninstalling mixless does not automatically remove its application support directory. Back up the library and any downloaded audio you want to keep before removing that directory. The Storage preferences provide controls for inspecting local data locations.

See the [privacy page](/privacy) for a complete explanation of network activity.
