# HID adapter ABI 1

The runtime is Wasmi 2.0.0, an interpreter. The desktop build enables validation
and bounded execution; it does not enable WASI or JIT compilation. The host
creates a separate instance and bounded message queue for each assigned
controller. Compiling and running guest code happen on that controller's worker.
Potentially blocking HID calls run on a separate device I/O worker. Audio
callbacks and UI rendering do neither.

HID enumeration, opening and report-descriptor reads use the persistent
discovery worker. After the initial inventory, the performance dispatcher polls
these requests without waiting for the OS to finish. A slow discovery or open
request does not stall already-connected controllers.

## Manifest and matching

`adapter.api_version` must equal 1. All logical endpoints must have HID matchers
with VID/PID, optionally interface, Usage Page and Usage. The user must assign
each interface explicitly. An adapter replaces the declarative `bindings`,
`feedback`, `initialize` and `shutdown` lists; those lists must be empty.

`controls` is an ordered allowlist of host targets. Indices below refer to that
list, not to the host's catalogue order. `reports` allows specific endpoint,
access (`output`, `feature_write`, `feature_read`), report ID and exact byte
length combinations. Every report starts with its ID byte, including a zero for
unnumbered devices. Input reports are normalized to the same convention.

## Exports

| Export | Signature | Meaning |
| --- | --- | --- |
| `memory` | Wasm linear memory | Input and output buffers |
| `mixless_buffer` | `() -> i32` | Address of a reserved 4096-byte input buffer |
| `mixless_event` | `(i32 kind, i32 endpoint, i32 ptr, i32 length, i64 time_ms) -> i32` | Process one event; return zero on success |

`mixless_buffer` runs during validation, without permission to emit effects.
The module must not have a Wasm start function. `time_ms` is a host monotonic
timestamp, not wall-clock time; compare differences rather than assuming that
the initial value is zero. Recorded timestamps are preserved during replay.
The input buffer is valid for the callback; copy data if it is needed later.

| Kind | Value | Endpoint / data |
| --- | --- | --- |
| Connect | 0 | `-1`, empty; all assigned interfaces are open |
| Input | 1 | Endpoint index, normalized input report |
| State | 2 | `-1`, empty; subscribed values or meters changed |
| Timer | 3 | `-1`, empty; the one-shot timer expired |
| Disconnect | 4 | `-1`, empty; advisory cleanup |
| Feature | 5 | Endpoint index, result of a feature request |
| I/O error | 6 | Endpoint index, empty; a requested read/write failed |

On physical disconnect, unassignment, overflow or a trap, the host independently
releases held controls and invalidates the instance's generation. Late actions
and queued adapter I/O from a retired instance cannot target its replacement.
Disconnect callbacks are best effort and their effects are discarded after
retirement; do not rely on a final USB write succeeding. Reinitialize the full
hardware state on the next Connect.

Hold ownership is shared with declarative MIDI/HID mappings. Releasing or
disconnecting one source does not release a target still held by another.
Hot cues and the temporary cue share a preview hold on each deck; the two decks
retain separate ownership. Reapplying a declarative preset initializes every
assigned endpoint and refreshes its feedback. Input queued before an assignment
change or restart is discarded for that controller, while other controllers
continue receiving their queued input.
Editing an explicit legacy wildcard mapping retires only its binding state and
queued wildcard input; device-specific bindings keep their state.
Queued declarative HID output is also cancelled on reassignment, and late
completion results cannot replace the new assignment's feedback error state.
Successful queueing alone does not clear a HID output error; a successful I/O
completion does.
Cleanup reports remain best effort and are ordered before the next initialization.

## Imports

All imports use the module name `mixless`. No filesystem, network, process,
device-opening or arbitrary command interface is linked.

| Function | Parameters / return | Meaning |
| --- | --- | --- |
| `emit` | `(i32 control, i32 kind, f64 value) -> ()` | Queue a validated host control action |
| `value` | `(i32 control) -> f64` | Current declared control value, normalized 0–1 |
| `meter` | `(i32 bus, i32 channel) -> f64` | 0–1 peak; bus 0 Master, 1 A, 2 B; channel 0 or 1 |
| `send` | `(i32 endpoint, i32 kind, i32 ptr, i32 length) -> ()` | Queue an allowed report; kind 0 output, 1 feature write |
| `read_feature` | `(i32 endpoint, i32 report_id, i32 length) -> ()` | Request an allowed feature report; result arrives in a Feature event |
| `timer` | `(i32 delay_ms) -> ()` | Replace the one-shot timer; 10–60000 ms |

The host accepts results from earlier callbacks before dispatching an expired
timer, because those callbacks can replace its deadline. This preserves the
callback order in recorded traces. A busy interpreter can delay a timer; timer
callbacks are not real-time audio clocks.

Software state is a snapshot for the current callback. Emitting an action does
not synchronously change that snapshot. State callbacks are coalesced at the
host feedback interval (33 ms); input dispatch is independent of that interval.
Implement button edges, duplicate suppression and output deduplication in the
adapter. The host caps output rate but does not reorder or deduplicate handshake
packets. Feature requests and replies are ordered on each interface.

`emit` value kinds:

| Kind | Value | Allowed target / value |
| --- | --- | --- |
| Press | 0 | Button targets; one action per intended press |
| Hold | 1 | Cue, Temporary Cue, Jog Touch or Brake |
| Release | 2 | Same hold-capable targets |
| Normalized | 3 | Continuous targets; finite 0–1 |
| Relative | 4 | Continuous controls or non-Jog encoders; integer −4096–4096 |
| Jog turns | 5 | Jog; finite −1–1 platter turns per event |

A platter turn corresponds to 1.8 seconds in the source sample domain, matching
the declarative jog convention. Convert device counts using its documented or
measured ticks per revolution. No imported adapter receives source audio.
Actions use the existing typed controller dispatch, including AutoMix takeover
and load ordering rules.

## Resource and failure rules

| Resource | Limit |
| --- | --- |
| Concurrent adapter instances | 16 |
| Binary module | 256 KiB; checksum required |
| Linear memory | One memory, 2 MiB |
| Table | One table, 1024 elements |
| Recursion | 64 frames |
| Value stack | 16384 entries |
| Execution | 50000 fuel units per callback |
| Effects | 64 per callback |
| Input callback rate | 4096 per second |
| Output / feature requests | 256 per second combined |
| Input report | 4096 bytes including ID |
| Output / feature request | Exact permitted length, at most 1024 bytes |
| Adapter queue | 256 events |
| Device I/O queue | 256 operations |

Wasmi's strict module parsing limits are also enabled. Fuel is an instruction
budget, not a wall-clock latency guarantee. Measure dispatch on the target host.
The interpreter isolates guest memory and capabilities inside the process; it is
not a separate OS process. Keep the runtime dependency maintained.

A callback's effects are committed only after it returns successfully. A trap,
invalid capability, invalid value, nonzero return or budget overflow discards
that callback's effects and stops the adapter. Previously held controls are
released. A fault in one adapter does not stop other controllers. A shared input
queue overflow releases all holds and stops all adapters; packets received
before recovery are discarded so an older press cannot reestablish a released
hold. The error appears in Controllers; applying
the preset again or physically reconnecting starts a fresh instance. Unsupported
versions, invalid hashes and invalid modules are rejected before import changes
the preset library. Failed persistence restores the in-memory library.

These contracts are independently defined by mixless. Runtime implementation
references: [Wasmi](https://github.com/wasmi-labs/wasmi/tree/v2.0.0),
[HIDAPI](https://libusb.info/hidapi/group__API.html).
