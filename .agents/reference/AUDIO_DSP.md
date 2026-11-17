> 当前 AutoMix 调整与验证见 [2026-09-13 记录](../history/2026-09-13-automix.md)。下方带日期的测量保留作历史背景。

# Audio DSP status and verification

The audio graph lives in `crates/mixless-engine`. UI gestures send controls;
they never synthesize PCM or own the audible playback position.

## Implemented signal path

Per deck: source-rate conversion + source-domain loop → independent music
tempo/key processing (or low-latency scratch resampling) → transport envelope →
phase-aligned LR4 three-band isolator → resonant channel filter → four stereo
insert effects → smoothed mixer TRIM → smoothed deck GAIN → per-deck
temporary AutoMix gain → stereo-linked sample-peak limiter → channel
fader/balance → smoothed crossfader.

Deck GAIN independently controls the limiter input from -12 to +12 dB; mixer
TRIM remains the channel calibration/AutoMix gain. Each deck limiter has a
fixed 0.98 sample-peak ceiling and an 80 ms release. PFL taps the limited signal
before fader/balance, and loading a new source clears old gain reduction while
preserving the deck GAIN setting. Master and headphone bus limiters still
protect their respective sums. These are sample-peak, not true-peak limiters.

Post-fader/crossfader sends feed shared wet-only Echo and Reverb returns. Those
returns keep processing after the send is closed, allowing the tail to decay.
The summed bus passes through master GAIN (-12 to +12 dB), five optional
parametric bell EQ bands, an optional stereo-linked soft-knee compressor, an
optional expander/gate, an always-active stereo-linked sample-peak limiter, then
master LEVEL (0 to 100%). LEVEL cannot drive the
limiter or undo attenuation: 25% output remains bounded by 0.245 even with
positive master GAIN. Signals below the limiter ceiling are not continuously
waveshaped. PFL remains independent of both master controls.

### AutoMix transition compensation

Dense-plan preparation renders an isolated host preview through the production
source, stretch, stem, EQ/filter, insert, limiter, fader, crossfader, send and
master graph. It measures 400 ms K-weighted loudness, 3 s short-term loudness,
phase-aligned 150 Hz / 2 kHz band powers and output true peak. Routed deck,
pre-master and final output measurements stay separate. The callback's normal
specialization adds no observation writes, metering, allocation or search.

A second preview supplies a full-source reference at the **same source clocks
and routing** with neutral channel tone and unity stems. This is a bounded
reference for compensation, not an alternative strategy. For full foreground
exchanges, robust local levels before and after the handoff define a slow
log-power trajectory between the two recordings. Each source's measured local
dynamics and spectral proportions multiply that trajectory,
so natural gaps/builds remain; no distant drop or track-wide target is substituted.
Partial crossfader landings or insufficient endpoint evidence keep the unshaped
reference. Joint fitting retains the measured cross terms between the routed decks. A missing stem can only earn positive gain on its own audible deck.
The band excess penalty limits amplification of the material that remains.
Cancellation does not authorize more gain. A correlated overlap can instead
receive up to 3 dB attenuation (the coherent equal-power midpoint excess).

Positive correction retains the 6 dB budget and 1 dB deadband. Both signs obey
remaining manual GAIN range and routed-share authority. Envelopes move at most
1.5 dB/s, additionally limited to 0.75 dB per sounding beat, and return to zero
at the handoff before any solo recovery tail. Source silence, near-empty stems,
closed channels, sub-two-second transitions and scratch gestures receive no
boost. These limits do not change cue, strategy, ordering, source clocks,
channel filter or crossfader. The immutable `MixPlan`, cue timing, ordering and planner
versions are unchanged; effective EQ and stem amplitudes may receive the bounded
offsets described below.

A third preview validates the proposed signed GAIN curve. It must improve the
measured routed sum without worsening the baseline-master-adjusted output
error, new meaningful band surges, extra deck/master limiting above 0.5 dB, or
an output true-peak increase beyond the bounded comparison tolerance. A failed
validation retains the exact plan with zero optional compensation. This does
not turn the sample-peak playback limiters into true-peak limiters.

Preparation has a ten-second wall-time limit and aborts when the moving outgoing
deck approaches launch within 250 ms. It publishes no partial curves. Manual
mixer, master processing, FX and transport changes invalidate the measurement,
including changes between preparation and publication. Skip, stop, source
replacement and handoff release compensation; pause suspends it. The existing
GAIN indicator displays either sign, independently of manual GAIN and source
normalization.

The preview uses a reproducible source pre-roll of up to three seconds and three
seconds after handoff. It is a measurement of that digital playback state, not a
clone of arbitrary live FX history or the hardware/acoustic output. Long-lived
pre-existing effect tails, manual changes, and subjective musical naturalness
remain separate validation boundaries. Real PCM renders and numerical agreement
do not establish listening acceptance. See
[the gain upgrade audit](../history/2026-11-17-automix-gain.md).

### Joint EQ and stem compensation

After validating broadband GAIN, preparation can refine low/mid/high isolator
levels and the four planned stem levels. The original envelopes remain stored;
EQ offsets add in dB and stem offsets multiply the original linear level. The
current protocol's 0..1 stem range and EQ -96..+12 dB range still apply. Muted
stems stay zero. An unavailable stem or a plan without stem lanes uses EQ only;
the LP/HP cutoff and resonance trajectories are not changed.

Small negative probes through the production graph measure each available
control's response on each routed deck. A bounded local log-power model jointly
fits GAIN, EQ and stems, including paired transfers between GAIN and a selective
control when the shared budget is full. This model proposes curves; it does not
replace PCM loudness measurements. Source-specific corrections have a lower
regularization cost than EQ affecting all sources in the same band.

Each EQ/stem offset is limited to 2 dB in either direction, reduced by routed
share. Every stem/band path shares the existing +6/-3 dB correction budget;
opposing corrections cannot create extra headroom. Existing broadband correction
can move toward zero to make room for selective correction. Tone offsets move
at most 1 dB/s and 0.75 dB per sounding beat, taper to zero, and cannot reverse a
moving base fade beyond control quantization. No new tone boost is authorized by
silence, a closed route, near-muted material or cancellation evidence.

Actual renders must improve the spectral objective against both the accepted
GAIN curve and the untouched plan without increasing broadband error. Final
output, short-term, band surge, limiter and true-peak checks remain mandatory.
An optimistic proposal may be retried at half and quarter strength. The ten-second
host budget includes probes and retries; exhaustion retains an already validated
GAIN curve if source/control/launch guards still hold. No probe runs on the callback.

Snapshots expose effective EQ/stem targets and separate automatic offsets.
Existing controls therefore start manual gestures at their effective values.
Cancellation smoothly releases the overlay; EQ takeover transfers the current
combined value into its release ramp so AUTO is not applied twice. Stem targets
continue using the existing 150 ms smoothing. Numerical validation and source
identity checks do not certify arbitrary live FX history or subjective sound.
See [the joint compensation audit](../history/2026-11-17-automix-tone.md).

Desktop publication avoids redundant loop-off commands, which are otherwise
treated as manual takeover and invalidate the prepared measurement. Replacement
preparation uses the candidate's beat grid and the incoming reset state. Its
commit applies that reset with the plan, preserving the control revision and
exact PCM/stem identity checks for intervening user edits. Signed gain/EQ/stem
slew limits share the available step across interpolated zero crossings.
Validation also limits additional contrast between adjacent measured output
windows to 0.75 dB for total loudness, all three bands and short-term loudness,
reserving 0.25 dB below the 1 dB audit boundary for window alignment and carried
DSP state in continuous playback.
The fitting grid remains about 200 ms. Validation reads the same 400 ms/3 s
loudness windows at four offsets (about 50 ms apart), and checks the same
200 ms band-power difference at every 32-frame offset. Band power uses a
rolling two-hop window rather than an average of the last two readouts, so
more frequent reads do not shorten its window. Boundaries are quantized to
audio blocks without accumulating drift at 44.1 kHz. All these measurements
run on the preparation thread; the playback callback only applies the curves.
This preserves existing transients while rejecting extra jumps caused by a
boosted source becoming dominant during routing or stem changes. Effectively
silent bands do not supply ratio evidence. GAIN candidates, like tone candidates,
can back off to one-half or one-quarter and must be rendered again within the
same preparation budget; otherwise the original plan plays without that layer.
See [the full-set review](../history/2026-11-17-automix-set-review.md).

Host parameter edits, source/stem publication and plan commit share the short
automation control gate. Cancellation and the parameter write cannot interleave
with commit's validation/reset. Preparation pins the control revision before
copying candidate settings and the source identities before compiling envelopes;
the expensive previews do not hold this gate. A late three-to-four-stem upgrade
invalidates the active compensation while retaining the musical plan. An explicit
Stop also invalidates in-flight measurements; the internal stop used to install
an exactly prepared replacement still relies on source identity verification.

Manual GAIN takeover transfers the current manual-times-AUTO product to one
smoother before releasing AUTO. This avoids a short boost or dip when dragging
from the displayed effective value. Other cancellation paths retain their normal
AUTO release. Preview copies the user's FX auto-fade setting; changes to that
setting or live roll invalidate prior measurements. Active live roll, brake,
reverse, loop, jog and cue-preview transport states bypass optional compensation
because the forward preview does not reproduce those states. Callback processing
continues to use nonblocking try_lock and allocation-free smoothing.
See [the lifecycle code review](../history/2026-11-17-automix-lifecycle-review.md).

Pre-transition performance gestures use `Engine::perform` for channel filter,
stem gain and EQ. These host-generated moves return to neutral before the armed
transition and do not mark its lanes or compensation as manually taken over.
The same EQ command sent through `dispatch` still applies manual takeover.
See [the follow-up review](../history/2026-11-17-automix-followup-review.md).

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
- Platter and waveform grabs, waveform wheel bursts and progress-ring drags
  use the same bidirectional scratch renderer. Paused audition is automatically
  sent pre-fader to monitor only, including release and insert tails, without
  changing PFL selection. Playing gestures retain normal master/PFL routing.
  No monitor device means no paused audition fallback to master.
- Holding still decays to silence; release preserves transport state. Wheel
  bursts release after 120 ms without events. Slip advances a background timeline
  for relative scrubbing. `ScrubTo` gives the progress ring an absolute target,
  auditions near distant destinations with the existing 6 ms crossfade, and
  commits the final position on release even with Slip enabled.
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


## Master mixer processors

`master/` owns preallocated stereo DSP. The EQ uses RBJ peaking coefficients
([W3C Audio EQ Cookbook](https://www.w3.org/TR/audio-eq-cookbook/)); its editor
calls the same coefficient response calculation. Frequency, gain and Q ramp over
20 ms; coefficients refresh every 32 samples. Bypasses and makeup crossfade over
10 ms. Filters use f64 state and flush denormals. Frequencies are capped at 45%
of the current sample rate in DSP.

Compression uses a linked sample-peak detector, a soft-knee transfer and
attack/release gain smoothing. The downward expander has bounded attenuation;
gate mode adds 3 dB closing hysteresis and a hold timer. All stages share gain
between channels. Limiting retains the previous default 0.98 ceiling and 80 ms
release; lowering the ceiling takes effect immediately, raising it ramps. There
is no lookahead delay, oversampling or inter-sample true-peak claim.

Host parameter writes are validated as complete typed settings. The callback
copies them via `try_lock` at block start and retains its prior settings when
busy; processing allocates nothing and never waits on that lock. Block peaks,
RMS and reduction are published as atomics. The desktop keeps a bounded history
only while the mixer is open. Preferences are written on gesture release, not
on every pointer move. Cue remains pre-master; recording remains post-master.

Tests cover default limiter compatibility, measured EQ response at 8–192 kHz,
compression ratio/makeup, expander range, gate hold, non-finite recovery, live
extreme edits, ceiling enforcement, settings rejection and busy-host fallback.
The full audio callback budget includes all three optional master processors.
Automated measurements do not establish subjective listening quality.
