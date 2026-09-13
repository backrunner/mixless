> 当前 AutoMix 调整与验证见 [2026-09-13 记录](../history/2026-09-13-automix.md)。下方带日期的测量保留作历史背景。

# Audio DSP status and verification

The audio graph lives in `crates/mixless-engine`. UI gestures send controls;
they never synthesize PCM or own the audible playback position.

## Implemented signal path

Per deck: source-rate conversion + source-domain loop → independent music
tempo/key processing (or low-latency scratch resampling) → transport envelope →
phase-aligned LR4 three-band isolator → resonant channel filter → four stereo
insert effects → smoothed trim/channel fader → smoothed crossfader.

Post-fader/crossfader sends feed shared wet-only Echo and Reverb returns. Those
returns keep processing after the send is closed, allowing the tail to decay.
The summed bus passes through master gain and a stereo-linked sample-peak
limiter. Signals below the limiter ceiling are not continuously waveshaped.

### Independent tempo and key

- Music mode uses the vendored Signalsmith Stretch 1.3.1 algorithm through a
  small stereo C++ bridge. Headers and MIT licenses are included in
  `crates/mixless-engine/native/vendor`; the build does not fetch native sources.
  macOS uses the Accelerate FFT backend.
- KEY LOCK defaults to on, with a per-deck toggle below SYNC. TEMPO changes
  source consumption and BPM without adding a pitch shift. KEY changes pitch
  in semitones without changing source duration, transport speed or BPM.
- With KEY LOCK off, tempo contributes the traditional vinyl pitch shift,
  `12 * log2(rate)`, in addition to the independent KEY offset. SYNC preserves
  the deck's current lock setting and KEY offset. FX tempo clocks follow
  source BPM × rate, never KEY.
- A 64 ms analysis window and 8 ms hop use split computation. At 48 kHz the
  input latency is 1536 frames (32 ms) and output latency is 1920 frames
  (40 ms). Decoded-file lookahead and pre-roll compensate startup/seek position;
  this does not remove the processor's control-response latency. Parameter
  changes enter 128-frame processing chunks, and source-advance metadata is
  delayed with the output so the visible playhead does not jump ahead on a
  tempo change. These are algorithmic figures, not measured device latency.
- Source sample-rate conversion occurs before stretching. Fractional input
  consumption carries a remainder instead of accumulating rounding drift.
  Loop reads wrap in the source domain; reverse music playback is supported.
- Unity, unshifted playback bypasses spectral processing, as does unlocked
  varispeed with zero KEY offset. Scratch always takes the resampling path.
  Music/scratch transitions blend over 6 ms. Cue, loop-range and direction
  changes retain a preallocated tail of the outgoing pitch-shifted audio
  before re-priming; returning from scratch primes at the audible position.

### Scratch and transport

- `SetJogTouch` acquires/releases the platter. `Jog` updates a bounded target;
  the audio thread follows it using a damped velocity controller instead of
  discontinuously writing the audible playhead from mouse events.
- Moving a paused deck is audible; holding it still decays to silence. Release
  preserves the transport state. Slip advances a separate background timeline.
- Cubic interpolation handles slow/reverse motion. A precomputed 48-tap,
  fractional-phase low-pass kernel bank reduces aliasing above normal speed.
  Rate and phase interpolation avoid hard switching between kernel banks.
- Cue, slip release and resampling-mode loop wrap use a multi-block 6 ms
  transition; music-mode loop reads wrap before stretching. Both
  sides advance during the transition; the previous position is not held as DC.
- Wave drags use window coordinates consistently. Window deactivation releases
  both scratch grabs and momentary FX.

### EQ and filter

- LR4 crossover points are 150 Hz and 2 kHz. The low branch is phase-aligned
  to the upper split, producing a flat summed magnitude at unity gain. This is
  an IIR crossover, not a zero-phase/bit-identical bypass.
- EQ, kill, gain and fader changes are smoothed. Kill is a gain target of zero,
  not a discontinuous change of DSP state.
- The channel filter uses topology-preserving state-variable filters. Turning
  the knob does not discard integrator history. The center becomes exact dry
  bypass after the short smoothing ramp.
- LP sweeps from 18 kHz to 30 Hz, HP from 30 Hz to 18 kHz; the upper endpoint
  is limited to 45% of the device sample rate. RES controls Q from approximately
  0.707 to 3.507 with moderate level compensation; double-click resets to 0.35.
- `SetFilter` honors cutoff Hz rather than mapping every LP/HP request to a
  fixed half-knob position. Filter snapshots expose the mapped cutoff and RES.

### FX

Each deck has four independent preallocated insert slots. The shared protocol
catalogue exposes **70 selectable types in 10 groups**; the desktop's `⋯`
button opens a grouped selector, description, beat controls and parameter knobs.
See [FX_CATALOGUE.md](FX_CATALOGUE.md) for sources, the full list and mappings.

| Family | Processing |
| --- | --- |
| Echo / Delay | Stereo feedback, ping-pong routing, filtered/saturated tape repeats, multi-tap patterns, pitch feedback and reverse chunks |
| Reverb | Four-line orthogonal feedback network; large-room reflections and freeze feedback |
| Modulation | Flanger/Jet, six-stage phaser, chorus, tremolo, auto-pan and three overlapping Mobius combs |
| Filter | Low/high/band-pass, beat sweep, envelope follower and two-formant vowel filter |
| Rhythm / Loop | Double-buffer capture, reverse order, repeat, stutter and slice permutations; independent live deck timeline |
| Pitch / Spectral | Signalsmith insert pitch, overlapping granular read heads, 12-band vocoder, ring modulation, tuned combs and 256-point spectral transform |
| Texture / Dynamics | Quantisation, sample hold, saturation, DC blocking, envelope compression and gate hysteresis |
| Noise / Sweep | Stereo PRNG noise with filter, gate, beat pumping, input-envelope following or one-shot rise |
| Release | Echo/reverb input closure, captured playback slowdown/backspin, one-shot and fade-out |
| Macro | One depth control combines filter, echo, room and noise |

Insert mix/bypass transitions use 5 ms ramps. HOLD restores the previous latch
state on release. Send returns are wet-only. Mix zero is dry for inserts and
silent for returns. Echo/reverb tails continue advancing while bypassed. A type
change first fades out, invalidates histories, then fades in. Capture resets
invalidate counters without clearing large sample banks. Unknown names and
non-finite controls are rejected by the host.

Pitch inserts use the already-vendored Signalsmith engine with a 32 ms window,
8 ms hop and 128-frame streaming blocks. They process equal input/output frame
counts and add wet-path latency (approximately 36 ms plus 128 frames at 48 kHz);
there is no source lookahead inside an insert. Granulizer uses overlapping
windowed read heads; Spectralizer uses four Hann overlaps with 256 frames of
latency. Wet latency is not compensated in the dry path.

Capture FX first collect the selected interval while passing live input, then
play the captured sound. Reverse/repeat/stop inserts do not alter the source
playhead: bypass returns to the continuing deck timeline. The separate source
transport Roll/Reverse/Brake commands retain their existing behavior. BEATS
limits capture and delay types to four beats (12 seconds at 20 BPM). Captured
interval length is latched until the next activation; a new beat setting takes
effect when rearmed. Release FX are rearmed by toggling OFF/ON or HOLD.

These are Mixless algorithms covering DJ audio effect categories. They do not
claim sample-identical recreation of proprietary presets or reproduce vendor
pad banks, Merge FX routing, video FX, Neural Mix stems or Audio Unit hosting.

## Held transport brake

During playback, holding Play past 400 ms starts a source-clock slowdown. For
elapsed audio time `t` in seconds, source speed is multiplied by
`0.03 + 0.97 / (1 + (t / 1.5)^2)`. The curve begins with zero slope, continues
slowing while held and approaches a positive speed so playback can still reach
EOF. It has no fixed stop timer and uses a 64-bit rendered-sample counter.
Key Lock and pitch processing are bypassed for the falling-speed sound; saved
tempo and key controls are unchanged.

Release stops the transport through its existing 1 ms de-click ramp, retaining
the last slowed speed through that tail. Window deactivation releases the hold.
EOF clears the brake, and a delayed release cannot restart playback or affect
a subsequently loaded track. A later Play starts with the saved tempo/key.
Fractional source steps also finish at the final sample instead of becoming
stuck there because the playhead is clamped below the old EOF comparison.


## Real-time work

Delay lines, reverb networks, resampling tables and stereo stretch processors
are allocated before the device stream starts, using the actual device sample
rate. No effect allocates
or rebuilds delay buffers in the callback. Control-to-gain conversion and FX
parameter reads happen at block boundaries, not once per audio sample. A
contended source lock uses the audio thread's existing buffer reference;
retired source buffers are reclaimed on the host thread at subsequent loads.

Scratch/Cut crossfader curves cut near the endpoints and keep both decks open
in the middle; they are not narrow center switches. Their gain ramps are 0.5 ms.
The I16 callback processes all chunks even if a device supplies more frames
than the preallocated conversion scratch buffer.

## Repeatable checks

On this machine, set `DEVELOPER_DIR=/Applications/Xcode-beta.app/Contents/Developer`.

```sh
cargo test -p mixless-engine
cargo test --workspace --exclude mixless-desktop
cargo test -p mixless-engine --release callback_budget -- --ignored --nocapture --test-threads=1
cargo check -p mixless-desktop
cargo build -p mixless-desktop --release
```

Tests cover independent tempo/key at 44.1/48/96 kHz (including source/device
rate mismatches), fractional transport drift, tempo-automation metadata delay,
pre-roll alignment, KEY LOCK/SYNC behavior, music/scratch/Cue/pause transitions,
loops and reverse playback, as well as filter stability, crossover gain,
resonance response,
effect bypass, echo timing and feedback, reverb decay, fast-scratch alias
rejection, forward/reverse/held/released scratching, slip, cue continuity,
source lock contention, and full effect-chain finite/bounded output.
The ignored benchmark renders 60 seconds per workload with preallocated output:
two decks with independent tempo/key offsets, eight inserts, filters and both
Echo/Reverb send returns, at 256 frames in music playback and 128 frames alternating scratch
with released music playback. Jog commands are emitted only while touching.
It reports p50/p99 DSP callback time separately from device latency, UI rendering
and OS scheduling. Its acceptance bounds are
50% and 35% of the respective block duration.

## Still requires separate work/acceptance

- The music stretcher is implemented, but synthetic tone tests do not certify
  commercial-grade sound over arbitrary tracks. Normal-range tone fixtures
  allow 0.2% frequency error; extreme tempo/key combinations allow 0.5%.
  Transient smearing, stereo texture, vocals and extreme settings still need
  listening acceptance. Formant preservation is not enabled by this bridge.
- The limiter bounds discrete sample peaks; it is not a lookahead/oversampled
  true-peak mastering limiter. The scratch filter is finite-length, not an
  ideal brick-wall anti-alias filter.
- Shared sends are post-fader/crossfader. Insert modulation follows the supplied
  deck beat grid; without a grid it uses the local tempo clock. Capture starts
  when activated; it is not quantized to the next grid boundary.
- Reverb is a compact FDN implementation, not a modeled room or convolution
  reverb. Echo variants share the same preallocated delay network, and the
  current catalogue does not attempt to reproduce vendor-specific presets.
- Hardware loopback latency, real-device xrun rates, headphones/PFL routing and
  subjective listening with representative tracks still require validation.
  Offline callback timing cannot certify audible quality or 60 fps UI rendering.


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
