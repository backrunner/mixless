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

Unless you have selected an order mode, AUTO uses **BEST** when a valid cached order is ready. On a cache miss, playback starts in the current playlist order while Best is calculated in the background. Completing that calculation keeps this session's order unchanged; the result is available at the next AUTO start or when you select **BEST**. During the current app session, each source playlist remembers its own **ORDER**, **SHUFFLE** or **BEST** selection, including **All Tracks**. Switching sources restores that source's choice or checks its own Best cache. Restoring **BEST** also requires a valid cache; a missing or outdated result falls back to **ORDER** and is recalculated in the background without changing this pass. Browsing with **Keep current** leaves the playing source's mode unchanged.

While **BEST** is calculating, the order button remains available. Switch to **ORDER** or **SHUFFLE** without waiting; the calculation continues to warm the cache and cannot change your selection.

AutoMix prepares its source playlist in the background. Each track’s entry, completed incoming mix and carried tempo/key constrain its exit. The list and playback share these plans; a new lap is planned separately so its entrance does not overlap the current lap’s exit.

In **Preferences → General → AutoMix**, **On playlist change** controls what happens when you select another playlist:

- **Follow selected** (default): use the newly selected playlist for upcoming tracks.
- **Keep current**: continue the current AutoMix playlist while you browse other playlists.

The choice is saved and applies to subsequent playlist selections, including **All Tracks**. Turning AUTO off and on starts from the playlist you are viewing. Edits to the running playlist still update its queue; deleting that playlist stops AutoMix.

## How transitions are chosen

mixless compares entry and exit regions using local tempo, phrase boundaries, key compatibility, energy, rhythm, and vocal evidence. Prepared stems and note analysis add information about foreground material and harmonic movement.

For overlaps, AutoMix checks the sounding keys in the transition windows, including carried pitch and effect tails. When the evidence supports a match, it transposes the incoming track before it becomes audible, choosing the nearest compatible key within two semitones of the recording and preferring a single semitone. The pitch stays fixed through the overlap. Vocal rides also compare the transposed notes. Keys further apart are not transposed; these and uncertain harmony use a separated drum layer, filtered transition, or cut instead.

A track played faster with **KEY LOCK** off also sounds higher; about 6% is one semitone. When the tempo an overlap holds the incoming track at is itself the transposition the keys need, within 10 cents, AutoMix plays that track with KEY LOCK off instead of stretching and shifting it. The deck's key badge shows the sounding key.

During solo playback, AutoMix can move the track back toward its original pitch in a measured, quieter passage. It considers the following track's key and departure window, then chooses a full or partial restoration with a smooth pitch curve, filter, EQ and available stems. Active vocals, held vocal notes, peaks and insufficient time prevent the move; sustained instrumental beds allow only a smaller, slower adjustment. Without a suitable window, the current pitch carries forward. Manual tuning stays authoritative. Taking over a masking control or pressing SKIP holds the pitch actually reached, which is used when planning the next handoff.

Beat-matching can leave the incoming track faster or slower than recorded. Once it plays alone, AutoMix glides it back to its own tempo at the slow rate used inside transitions; a 12% change takes about a minute. It keeps part of the change, up to 6%, only when the following track needs a move in the same direction. A short remaining track gets a partial correction. Changing the tempo yourself takes over, and SKIP moves straight to the tempo the glide was heading for.

A track playing with KEY LOCK off returns all the way to its own tempo, and its pitch returns with it at no more than about 6 cents a second. KEY LOCK switches back on once the track is at its recorded speed. SKIP during this glide holds the tempo and pitch reached, with KEY LOCK left off.

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

When tempos are too far apart to beat-match, prepared stems let AutoMix fit separate entrance and exit curves for each part. It searches for an incoming downbeat, uses the measured remaining parts to carry the rhythm change, and adjusts each deck's level and EQ independently. Unsupported tonal overlaps and competing drum patterns stay separated. A transition name does not select a fixed set of curves; insufficient evidence keeps the conservative handoff available.

## Take control

**PAUSE**, **RESUME**, and **SKIP** are available while a plan runs. Adjusting a fader, EQ, or filter takes over that lane only. Loading a track manually takes over from AutoMix.

AutoMix can add temporary gain during a sustained overlap that loses energy. The deck's **AUTO +…** display shows this compensation; manual changes take control and compensation returns to zero after the mix.

Measured frequency balance also supports gentle, cut-only EQ: up to 1.5 dB of section correction during solo playback with Live moves enabled, and up to 2 dB of tonal correction during an overlap. Missing spectral evidence leaves this correction neutral. Bass swaps retain their intended low-frequency handoff.

The **Live moves** preference (Off / Subtle / Active) controls extra performance gestures. Subtle uses gentle filter rises only at measured builds and drum dips of up to 3 dB at measured, low-vocal phrase boundaries. Active permits stronger moves and automatic spinback accents. Gestures return smoothly to neutral before the transition, and touching a control hands it back to you for the rest of the track.

For the rest of the controls, see [decks and effects](/docs/decks) and [audio levels](/docs/audio).

Changing playlist with **Follow selected**, editing the running playlist’s order, or completing its downloads, analysis or stem preparation replans the remaining queue. When there is time to prepare a safe handoff, the idle deck switches to the new candidate while the current song continues. Once a mix has begun, or a replacement cannot be ready safely, that handoff finishes and the requested candidate takes the next slot.
