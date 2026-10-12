# NI Traktor source inspection

This directory contains an independently authored, read-only developer tool and
a source audit of the installed Traktor Pro 4.4.1 build 22. It is not a controller
preset, report fixture or hardware verification record. No vendor software,
mapping logic or binary payload is redistributed.

`traktor_inspect.py` reads files only. It does not enumerate/open controllers,
start Traktor or Controller Editor, load NI libraries, or send USB/MIDI messages.
Python 3 is the only dependency.

```sh
python3 -B traktor_inspect.py tsi '/path/to/NI/controller.tsi'
python3 -B traktor_inspect.py tsi '/path/to/NI/controller.tsi' --controls
python3 -B traktor_inspect.py audio '/path/to/Traktor executable'
python3 -B traktor_inspect.py audit '/path/to/Factory Settings/4.4.1' '/path/to/Resources/qml/CSI' '/path/to/Traktor executable'
python3 -B -m unittest discover -s . -p 'test_*.py'
```

Run these commands from this directory. Output is JSON on stdout. `audit` scans
only the NI factory folders and nine explicitly identified NI CSI directories;
it does not inspect third-party controller mappings. The checked-in audit omits
control-name lists, port names, serial fields and command assignments. Source
paths are relative and vendor file hashes allow later comparison.

The observed TSI layout is an XML `DeviceIO.Config.Controller` entry whose base64
value contains length-delimited `DIOM`, `DEVS` and `DEVI` chunks. Device classes
use length-prefixed UTF-16BE. `DDAT/DDDC/DDCI` and `DDCO` contain counted `DCDT`
descriptor lists. The tool checks lengths, counts and parent boundaries, rejects
entity declarations and bounds file reads. Unknown optional device data is
skipped. Port/serial (`DDPT`) and command-assignment (`DDCB`) payloads are not
decoded. The format details were independently inspected from the vendor files;
tests construct fictional descriptors and opaque assignment data.

The source audit establishes these distinctions:

- X1 has 120 input and 88 output descriptors in each of five native layouts.
- F1 Track Deck has 42 input and 271 output descriptors; the sequencer and
  Pattern Player definitions have 42 inputs and 269 outputs.
- S2 User has 81 input and 52 output descriptors; S4 User has 128 and 108.
  Their class strings do not distinguish hardware generations.
- One X1 Presets file contains settings only and no controller definition.
- Native descriptor names refer to software controls, not USB packet fields.
  Descriptor counts include alternate modes and do not count physical controls.
  No `DCDT` numeric field is assumed to be a MIDI number or report offset.

The executable's embedded XML contains independent per-device internal-mixer
output pairs for 13 NI DJ controllers. Duplicate architecture copies must agree
before a result is emitted. These numeric facts back mixless's explicit audio
recommendations, with a test comparing each supported name and pair to this
audit. They do not configure sample rate, buffering, microphone inputs, hardware
mixing or any controller protocol. The executable is never loaded or executed.

See [NI_COMPATIBILITY.md](../../NI_COMPATIBILITY.md) for source boundaries and
remaining packet, initialization, feedback and physical-validation gaps.
