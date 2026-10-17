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

Click a deck’s time to switch between remaining time and elapsed playback time. Each deck remembers its choice; its waveform time uses the same mode.

**KEY** changes pitch independently of tempo. The key badge shows the sounding key after transposition; with **KEY LOCK** off, it also includes the pitch change from tempo. Fractional tuning is shown in cents.

## Cue points

There are eight saved hot cues per track. Press an empty pad to save the current position; an existing pad jumps there on mouse-down without changing playback. Right-click to delete. **G** fills the next empty slot without replacing existing cues.

Assign AUTO, IN, or OUT roles with **Shift + cue**. See [AutoMix](/docs/automix) for how these affect transitions. Saved cues persist with the track.

Transport **CUE** is separate: the first press saves a temporary deck-local point, later presses return immediately, and holding for 400 ms clears it. A **T** flag marks it in the scrolling waveform. Loading a different track clears it.

## Loops and vinyl control

Toggle a loop with **L**, and halve or double its length with **[** and **]**, from 1/16 to 64 beats. Drag the platter to jog. Set **General → Deck playback → Jog sensitivity** from 0.25× to 4×; 1× moves 1.8 seconds per revolution. This setting applies to mouse gestures; MIDI jogs use their own mapping calibration.

Click or drag the outer progress ring to seek through the whole track. The top is the start, and progress runs clockwise. Dragging past the start or end stops there. Seeking preserves playback or pause, and takes over from AutoMix and sync.

A short Play click stops on release. Holding Play for 400 ms begins a continuous vinyl slowdown with falling speed and pitch, even with Key Lock on. Release stops playback; reaching the end of the track stops it naturally. Press Play again to resume at the saved tempo and key settings.

## Waveforms

Spectral colors distinguish bass (red), low-mid (yellow), high-mid (green), and treble (blue). Numbered flags locate hot cues. The library presents full-track waveforms; the decks provide the detailed playback view.

Drag or scroll a deck waveform to move through the track. A mouse wheel works in both layouts; the top layout also accepts horizontal trackpad scrolling. Scrolling uses the same time scale and content direction as dragging.

Platter, waveform and progress-ring drags, as well as waveform scrolling, audition the audio as you move. While paused, this audio goes only to the monitor output, before the channel fader, without changing PFL selection. While playing, it follows the normal Master routing. Releasing the gesture preserves the playback state; wheel audition ends after scrolling stops. Configure a monitor output to hear paused audition.

The MIDI wireframe exposes **Track position** on the outer ring and **Elapsed / remaining time** in the deck header. **Jog rotation** remains a separate control.

Choose waveform placement and visibility in General preferences. Artwork appears in the platter hub; missing artwork uses the default platter.

## Stem controls

Prepared stems expose **VOCAL**, **DRUMS**, **BASS**, and **OTHER** gain in a 2×2 control grid. They share the deck's transport. With all voices at unity, the original audio is preserved. Preparation happens locally and can continue while you play.

## Effects

Each deck has four insert slots and a catalogue of 70 effects. Open **⋯** on a slot for grouped effects, beat timing, and parameter editing. Echo, reverb, filters, and rhythmic effects can help shape a handoff; use the level controls to manage the result.

Quantize, Key Lock, vinyl/slip, filter/EQ resonance, FX visibility, and crossfader curve/reverse are saved in General preferences. See [audio setup](/docs/audio) for gain and headphone monitoring.


## Master mixer

Click **MASTER** in the toolbar to open the master mixer. The signal runs through input GAIN, EQ, Compressor, Dynamics, Limiter, then output LEVEL. Headphone cue monitoring stays independent; recording receives the processed master output.

- **Parametric EQ:** five bell bands with frequency, gain and Q. Drag a point to adjust its frequency and gain, or select a band and use its knobs. The curve shows the digital filter response.
- **Compressor:** threshold, ratio, knee, attack, release and makeup gain, with an input/output curve and live gain reduction.
- **Dynamics:** choose Expander or Gate. Set threshold, range, attack, hold and release; the expander also has a ratio. Gate adds 3 dB closing hysteresis to reduce chatter.
- **Limiter:** always active, with adjustable ceiling and release, input/output peak and output RMS readings, and five seconds of level and reduction history. It is a stereo-linked, zero-lookahead sample-peak limiter; it does not measure or limit inter-sample true peaks.

EQ, Compressor and Dynamics start off and can be enabled independently. Processor settings are saved when you release a control. Double-click a knob to restore its default, or use **Reset processors** to restore the chain. **Esc**, **Close**, or a click outside the panel closes it without interrupting playback.
