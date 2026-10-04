---
title: "Audio & monitoring"
description: "Set up master and headphone outputs, then balance your levels."
order: 5
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

Open **mixless → Preferences…** with **Cmd+,** and choose **Audio I/O**.

## Output devices

Select a master output, sample rate (up to 192 kHz where supported), output bit depth, and buffer size. **Apply audio** reopens the streams, briefly interrupting playback. An unavailable device or unsupported format reports an error and retains the previous configuration.

Sample rate and output bit depth default to **System / device current**, reading the device's current physical format when the output opens or you apply settings. On macOS, system format changes are checked once per second and the streams reopen to match; reconfiguration waits until recording stops. Manual settings change the CoreAudio hardware format and verify it by reading it back. The bit-depth menu lists only depths supported at the selected sample rate; a failed change restores the previous hardware format. The master sample rate and depth do not change the separate headphone device, which follows its own physical format.

Sources are converted to the output sample rate while preserving playback duration and pitch at normal speed. Mixing and effects use 32-bit float internally, with final conversion to the physical output format handled by CoreAudio. **Current audio** shows the physical master/headphone formats separately from the processing stream. Increasing sample rate or bit depth does not recover detail absent from the source, and this mixer does not provide bit-perfect playback.

macOS output uses CoreAudio and defaults to **Low latency (auto · 256 frames)**. This requests 256 frames independently for master and headphone outputs, choosing the nearest size within each device's supported range when needed. If the range is unknown, the device default is retained. Existing manual buffer settings are preserved.

**Buffer size** offers 64, 128, 256, 512, 1024, or 2048 frames for both master and headphone outputs. A manual size must be supported by both enabled devices. **Current audio** shows the master output's actual callback size and buffer duration. At 48 kHz, 256 frames last about 5.33 ms; device, wireless transport, and audio processing delays add to total output latency.

A smaller buffer reduces latency but leaves less time to process audio. If playback breaks up, increase the buffer and check the device's supported settings.

## Headphone cue

Select a separate headphone output for PFL. The pre-fader headphone bus is independent of channel faders and master volume, including when the two devices use different sample rates. Headphone volume has its own control.

Without a headphone output, PFL is disabled. Hot cues remain available: hot cues move playback, while PFL monitors a channel.

## Gain and level

| Control | Purpose |
| --- | --- |
| Mixer TRIM | Calibrate the channel's level |
| Deck GAIN | Drive that deck's limiter, from −12 to +12 dB |
| Channel fader | Set the level after deck limiting |
| Master GAIN | Drive the master limiter |
| Master LEVEL | Set final output volume after limiting |
| Headphone volume | Set independent headphone output level |

PFL monitors the limited deck signal before its channel fader. Master GAIN and LEVEL do not change headphone volume.

AutoMix may temporarily add up to 6 dB when a sustained overlap loses energy. **AUTO +…** shows the compensation. Your manual gain change takes control, and the temporary offset returns to zero after the mix.

## General preferences

General saves waveform layout, effects visibility, quantize, Key Lock, vinyl/slip, crossfader curve/reverse, and filter/EQ resonance. Changes also apply to the current session.

![mixless General preferences](/images/preferences.webp)

Microphone and Line In capture are not implemented. For controller setup, continue to [MIDI mapping](/docs/midi).
