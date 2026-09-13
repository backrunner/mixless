> 当前 AutoMix 调整与验证见 [2026-09-13 记录](../history/2026-09-13-automix.md)。下方带日期的测量保留作历史背景。

# DJ FX catalogue — 2026-09-06

Mixless now exposes 70 audio effect types across ten groups, through all four
insert slots on each deck. Click the slot's **⋯** button to select a type and
adjust its parameters. Clicking the name latches the effect; **HOLD** applies it
until release. The list is the Mixless implementation catalogue, not a claim
that each vendor has exactly 70 independent algorithms.

## Reference products and sources

- [Rekordbox 7.2.18 manual](https://cdn.rekordbox.com/files/20260807093645/rekordbox7.2.18_manual_EN.pdf), printed pages 181–194: Beat FX, Sound Color FX, Release FX, PAD FX and Merge FX. PAD FX is a way of triggering effects; Merge FX combines build, release and drop stages. The manual documents these modes rather than a complete text list of every selectable preset.
- [djay Pro for Mac: Audio effects](https://help.algoriddim.com/user-manual/djay-pro-mac/dj-tools/effects/audio-effects): reverbs, echoes, sweeps, macros, filters and modulation.
- [djay effects overview](https://help.algoriddim.com/user-manual/djay-pro-mac/dj-tools/effects): audio, video and audio-visual effects are separate capabilities.
- The installed **djay Pro 5.6.8** bundle was also inspected read-only for names/categories. Its effect pack labels include Essentials, Reverb & Echo, Noise & Sweep, Macro Up/Down, Filter, Cut, Slice, Juggle, Resonate, Modulate and Warp. Some labels are internal pack names, not a promise that every edition exposes them. No vendor DSP code or preset files were copied into Mixless.

## Coverage by category

| Reference category / behavior | Mixless equivalents |
| --- | --- |
| Beat FX: echo, delay, pitch and modulation | Echo, Delay, Ping Pong, Low Cut Echo, Spiral, Pitch Delay, Flanger, Jet, Phaser, Chorus, Pitch, Mobius variants |
| Color FX: filter, dub echo, noise, crush, space | Low/High/Band Pass, Sweep Filter, Dub Echo, Noise/Sweep, Bitcrusher, Space |
| Beat FX / pads: Trans, Roll, Slip Roll, reverse/repeat | Gate / Trans, Transform, Roll, Slip Roll, Loop Roll, Reverse Roll, Stutter, Slicer |
| Release: stop, backspin, echo/reverb out | Vinyl Brake, Tape Stop, Backspin, Echo Out, Reverb Out |
| djay Reverb & Echo / Resonate | Room, freeze, tape/pattern/pitch delays, resonator |
| djay Noise & Sweep / Filter | Noise, gated/pumped/following noise, riser, LP/HP/BP, envelope filter, vowel filter |
| djay Modulate / Warp | Modulation, ring modulation, vocoder, granular/spectral processing, distortion, bit reduction |
| djay Slice / Juggle / Cut | Captured repeats, slice permutation, beat gate, stutter, sidechain ducking |
| djay Macro / Rekordbox Merge-style combined sound | Build / Drop combines a filter sweep, echo, room and noise under one depth control |

The combined-sound row covers an audio macro, not the full Rekordbox Merge FX
state machine and drop-sample workflow. Vendor-specific presets such as djay's
Hydrant or Lunar Echo are not advertised as exact replicas. Different names
can share a DSP core; for example, Censor and Reverse Roll use backwards
capture playback, and Spiral/Pitch Delay use pitch-shifted feedback.

## Complete Mixless list

| Group | Types |
| --- | --- |
| Echo & Delay | Echo, Ping Pong, Dub Echo, Delay, Space Echo, Low Cut Echo, Spiral, Helix, Tape Echo, Pattern Delay, Reverse Delay, Pitch Delay |
| Modulation | Flanger, Phaser, Chorus, Tremolo, Auto Pan, Mobius Saw, Jet, Mobius Triangle |
| Rhythm & Loop | Gate / Trans, Roll, Slip Roll, Loop Roll, Beatmasher, Stutter, Beat Loop, Transform, Reverse, LFO Chop, Slicer, Auto Sidechain, Censor, Reverse Roll |
| Reverb | Reverb, Space, Freeze Reverb |
| Filter | Sweep Filter, Auto Filter, Low Pass, High Pass, Band Pass, Vowel Filter |
| Texture & Dynamics | Bitcrusher, Distortion, Compressor, Fuzz, Overdrive, Noise Gate |
| Noise & Sweep | Noise, Riser, Noise Sweep, Gated Noise, Noise Pump, Noise Follower |
| Release | Echo Out, Vinyl Brake, Tape Stop, Backspin, One Shot, Fade Out, Reverb Out |
| Pitch & Spectral | Pitch, Ring Mod, Resonator, Vocoder, Granulizer, Spectralizer, Robot |
| Macro | Build / Drop |

## Implementation and validation boundaries

The catalogue/state (`mixless-protocol/src/fx/`), DSP cores
(`mixless-engine/src/effects/`) and controls/editor (`apps/desktop/src/fx/`)
have separate module boundaries. See [CONTRIBUTING.md](../../CONTRIBUTING.md#module-boundaries)
for the extension points and required checks when adding another effect.

The actual algorithms, routing, latency and capture/rearm behavior are recorded
in [AUDIO_DSP.md](AUDIO_DSP.md). This implementation adds audio inserts and a
selector/editor. Video mixing, spatial output, stem separation, AU plug-in
hosting and vendor preset packs are separate features.

Checks cover pitch-frequency changes and stereo energy at 44.1/48/96 kHz,
reverse sample order, captured-loop repetition, brake speed decay and rearming,
FFT reconstruction, band-pass rejection, frozen-room energy, release input
closure, every type at parameter extremes, bypass and wet-only sends. The
existing engine tests cover transport, sync, automation and limiter behavior.

After module extraction, the local release benchmark on macOS 27.0 / arm64
measured the slowest of 70 individual inserts at **0.0653 ms p99 per 256 frames /
48 kHz** (Pitch), below the 0.12 ms target. The two-deck baseline measured
0.342 ms p99 at 256 frames and 0.161 ms at 128 frames. These are offline callback
measurements, not device latency or listening
acceptance. Synthetic tests do not certify perceptual equivalence to commercial
FX, and arbitrary music still needs listening validation.


## FX wet/dry transitions (2026-09-13)

FX fade in / out is enabled by default in Playback preferences and persisted.
Tail-bearing effects (including reverb, echo and delay) move their wet/dry mix
through a 150 ms sample-clocked smoothstep envelope; other inserts use 35 ms.
Disabling the preference retains the 5 ms click-prevention envelope. Retriggering
starts from the current wet level. Block parameter refreshes do not restart a
fade; existing delay/reverb state keeps decaying through release. Both deck
inserts and master sends use the same policy, without callback allocation, I/O
or locks. Effect-type changes fade the previous type out before its DSP reset.

Validation and musical-analysis limits are recorded in [AutoMix continuity](../history/2026-09-13-automix.md).
