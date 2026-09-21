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

Selecting a playlist prepares analyses and adjacent transitions in the background, including the last-to-first handoff. Tracks added during an import join the queue at a later handoff.

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

Between transitions, the **Live moves** preference (Off / Subtle / Active) lets AutoMix add small analysis-driven gestures to the playing track — filter risers into drops and brief drum stem pull-outs inside them. Touching the filter or a stem control hands that control back to you for the rest of the track.

For the rest of the controls, see [decks and effects](/docs/decks) and [audio levels](/docs/audio).
