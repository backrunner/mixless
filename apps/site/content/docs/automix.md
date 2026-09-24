---
title: "AutoMix"
description: "Build a continuous set while keeping control of each transition."
order: 3
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

## Start and sequence

Select a playlist and enable **AUTO**. With empty decks, the first playable song starts in list order, even with shuffle enabled. Subsequent songs follow sequence or shuffle. Each shuffle cycle visits every entry before reshuffling, and the playlist loops continuously.

Selecting a playlist prepares the set in the background. Each track’s entry, completed incoming mix and carried tempo/key constrain its exit. The list and playback share these plans; a new lap is planned separately so its entrance does not overlap the current lap’s exit.

## How transitions are chosen

mixless compares entry and exit regions using local tempo, phrase boundaries, key compatibility, energy, rhythm, and vocal evidence. Prepared stems and note analysis add information about foreground material and harmonic movement.

| Technique | Musical purpose |
| --- | --- |
| Phrase blend | Align phrases and gradually exchange space between two playing tracks |
| Bass swap | Hand the low end from one track to the other during an overlap |
| Drop cut | Resolve an identified build-up into the next track's drop or chorus |
| Echo or filter exit | Shape the outgoing track around the handoff |
| Structural cut | Switch at an appropriate boundary when an overlap is unsuitable |

Transition length follows the music. A suitable pair can use a long layered blend; a structural cut can be instantaneous. A Drop Cut requires evidence of a real build-up ending and a suitable incoming section. Where the evidence supports it, a transition can also finish with a **Spinback** (a short accelerating backspin on the outgoing deck) or **Loop out** (the outgoing track loops its final phrase while stems or EQ strip it down underneath the incoming one). Not every pair should use the same technique.

Automatic exits require a detected section edge, a phrase boundary supported by measured change, or the end of the recording. Counting a fixed number of bars alone does not authorize a cut. Incoming energy is checked against the outgoing section so a strong handoff does not land in a quiet fade-in. Your manual IN/OUT cues keep priority.

Analysis is an estimate. Uncertain beat grids, dense vocals, or incompatible structure can lead to a conservative handoff. Listen to unfamiliar pairs before relying on them in a set.

## Set your boundaries

Hold **Shift** (or click the deck's **SHIFT** control) and choose a saved hot cue to assign its role:

- **AUTO**: a normal cue, available as a soft anchor.
- **IN**: a required incoming mix point.
- **OUT**: a required outgoing mix point.

Changing the role keeps the cue's saved position. Manual IN/OUT cues are hard constraints; an impossible window is reported instead of silently ignored. Analysis fills unused pads without replacing manual edits.

## Follow the handoff

The transition strip shows the selected windows, countdown or progress, technique, and incoming controls. Corresponding regions are highlighted on the waveforms.

The idle deck is prepared with its cue, tempo, key lock, EQ, and a closed level. During the handoff, AutoMix can drive transport, filter, EQ, channel faders, crossfader, selected effects, and stem levels.

## Take control

**PAUSE**, **RESUME**, and **SKIP** are available while a plan runs. Adjusting a fader, EQ, or filter takes over that lane only. Loading a track manually takes over from AutoMix.

AutoMix can add temporary gain during a sustained overlap that loses energy. The deck's **AUTO +…** display shows this compensation; manual changes take control and compensation returns to zero after the mix.

Measured frequency balance also supports gentle, cut-only EQ: up to 1.5 dB of section correction during solo playback with Live moves enabled, and up to 2 dB of tonal correction during an overlap. Missing spectral evidence leaves this correction neutral. Bass swaps retain their intended low-frequency handoff.

The **Live moves** preference (Off / Subtle / Active) controls extra performance gestures. Subtle uses gentle filter rises only at measured builds and drum dips of up to 3 dB at measured, low-vocal phrase boundaries. Active permits stronger moves and automatic spinback accents. Gestures return smoothly to neutral before the transition, and touching a control hands it back to you for the rest of the track.

For the rest of the controls, see [decks and effects](/docs/decks) and [audio levels](/docs/audio).

Changing playlist or order, or completing downloads, analysis or stem preparation, replans the remaining queue. When there is time to prepare a safe handoff, the idle deck switches to the new candidate while the current song continues. Once a mix has begun, or a replacement cannot be ready safely, that handoff finishes and the requested candidate takes the next slot.
