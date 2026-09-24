---
title: "Decks, cues & effects"
description: "Use the dual decks, shape stems, and work with your beat grids."
order: 4
image: /images/social.png
imageAlt: "mixless dual-deck workspace"
imageWidth: 1200
imageHeight: 630
imageType: image/png
---

## Transport and sync

The highlighted deck receives selected-deck commands. **Tab** switches between A and B; **Space** toggles the selected deck's playback.

**SYNC** aligns the follower to the reference deck. The interface distinguishes **MASTER**, **SYNC …** (acquiring), a locked **SYNC**, and **GRID … / NO GRID**. You can request sync while analysis is running; it engages when both grids are ready. Press again to cancel.

Manual scrubbing, reversing, or changing the follower's tempo takes control of timing. Loading another track cancels a pending sync request.

## Cue points

There are eight saved hot cues per track. Press an empty pad to save the current position; an existing pad jumps there on mouse-down without changing playback. Right-click to delete. **G** fills the next empty slot without replacing existing cues.

Assign AUTO, IN, or OUT roles with **Shift + cue**. See [AutoMix](/docs/automix) for how these affect transitions. Saved cues persist with the track.

Transport **CUE** is separate: the first press saves a temporary deck-local point, later presses return immediately, and holding for 400 ms clears it. A **T** flag marks it in the scrolling waveform. Loading a different track clears it.

## Loops and vinyl control

Toggle a loop with **L**, and halve or double its length with **[** and **]**, from 1/16 to 64 beats. The platter supports scratching.

A short Play click stops on release. Holding Play for 400 ms begins a continuous vinyl slowdown with falling speed and pitch, even with Key Lock on. Release stops playback; reaching the end of the track stops it naturally. Press Play again to resume at the saved tempo and key settings.

## Waveforms

Spectral colors distinguish bass (red), low-mid (yellow), high-mid (green), and treble (blue). Numbered flags locate hot cues. The library presents full-track waveforms; the decks provide the detailed playback view.

Choose waveform placement and visibility in General preferences. Artwork appears in the platter hub; missing artwork uses the default platter.

## Stem controls

Prepared stems expose **VOCAL**, **DRUMS**, **BASS**, and **OTHER** gain in a 2×2 control grid. They share the deck's transport. With all voices at unity, the original audio is preserved. Preparation happens locally and can continue while you play.

## Effects

Each deck has four insert slots and a catalogue of 70 effects. Open **⋯** on a slot for grouped effects, beat timing, and parameter editing. Echo, reverb, filters, and rhythmic effects can help shape a handoff; use the level controls to manage the result.

Quantize, Key Lock, vinyl/slip, filter/EQ resonance, FX visibility, and crossfader curve/reverse are saved in General preferences. See [audio setup](/docs/audio) for gain and headphone monitoring.
