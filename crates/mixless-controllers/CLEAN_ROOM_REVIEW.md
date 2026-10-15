# NI implementation and clean-room review

Reviewed 2026-10-12. Scope: the pending NI controller, Learn/calibration, capture,
headphone-mix and Traktor-source-audit changes in this working tree.

## Finding

The current work is recorded as **independently authored implementation with
vendor-derived interface facts**. It is **not verified strict clean room** and
has **no legal-clearance claim**. Tests do not prove that code has no bugs, and
artifact checks do not establish the legality of every research activity.

The same implementation agent inspected Controller Editor templates, Traktor
QML, executable symbol names and embedded configuration before implementing
the changes. There was no separation between researchers and implementation
authors, nor an independently reviewed specification handed to an unexposed
implementer. This exposure cannot be undone by moving files or rewriting their
provenance. Historical Mixxx-derived presets were removed previously; this
review does not rewrite or certify the repository's history.

The machine-readable [NI provenance record](NI_PROVENANCE.json) retains that
status and the source-to-implementation relationships. Its statements describe
the reviewed changes, not every possible third-party contribution.

## Sources and delivery

| Material reviewed | Information used | Delivery boundary |
| --- | --- | --- |
| Named Controller Editor templates | MIDI addresses, gate values and encoder encoding | Two independently chosen mixless layouts; original templates are not bundled/imported |
| Traktor embedded audio XML | Exact device names and output-channel pairs | Explicit audio proposals; no Traktor auto-configuration algorithm, rates or gain settings |
| Traktor QML and executable symbols | Behavior observations and existence of an NHL abstraction | Research notes; no NI native adapter, proprietary-library bridge or copied QML |
| Traktor TSI descriptor containers | Device classes and descriptor counts | Read-only developer inspector; command assignments and vendor mapping logic are skipped |
| MIDI/HID standards and project requirements | Framing and required application behavior | Independently authored runtime, Learn, calibration, capture and Cue/Master interpolation |

NI fixtures use independently constructed messages and expected mixless actions.
They are not hardware captures or translated vendor tests. Source hashes and
counts remain in the audit; original QML, TSI, templates and vendor executable
payloads are absent from the controller source delivery. Product code does not
read the installed Traktor files or require a running Traktor process.

`scripts/check-controller-provenance.py` scans the current Git delivery for
vendor container/source formats, native executables in the controller tree,
specific vendor-loading/source-access primitives, unexpected source-audit fields
and inaccurate clean-room status. CI also runs the inspector's synthetic tests.
The guard cannot detect every form of disguised copying, check licenses or
verify researcher independence. It is a regression check, not a certification.

## Rights and process limits

NI's [current EULA](https://www.native-instruments.com/pages/end-user-license-agreement)
(2026-07-01), section 3.1, contains restrictions concerning copying, reverse
engineering and AI-related use. The locally installed Traktor EULA file redirects
to NI's online agreement; this review did not establish the user's governing
license version or an authorization for every inspection activity.

Interoperability provisions also have conditions: see
[US 17 USC 1201(f)](https://www.copyright.gov/title17/92chap12.html) and
[EU Directive 2009/24/EC, articles 5–6](https://eur-lex.europa.eu/eli/dir/2009/24/oj).
Their applicability, contract effects and other rights are not decided by this
code review. Uncopied code and functional facts alone are insufficient grounds
for declaring the whole process legally cleared.

For a future strict separation process, document a reviewed factual interface
specification with source permissions, assign implementation to an author who
has not accessed vendor implementation material, and retain distinct access,
authorship and review records. Previously exposed implementation cannot be
retroactively labelled clean room; affected code would need its own independent
review/reimplementation process. Do not certify separation merely by starting
another agent or clearing its context.

## Code findings corrected

- Relative sensitivity below one discarded fractional non-jog steps. Per-binding
  accumulation now preserves movement and resets on disconnect/layer changes.
- Shortcut Learn discarded explicit CC14 pairing for existing mappings. Normal
  and shortcut Learn now use the same arming path.
- CC14 learning retained a pre-disconnect half/result. Retiring a device now
  clears its Learn decoder and captured result before accepting new input.
  Arming Learn releases performance state separately: post-arm halves/results
  survive that transition, while timestamped pre-arm packets are ignored.
- Editing message type, channel or CC halves left an old Learn operation armed.
  Manual address/preset changes now cancel that operation.
- Legacy last-message diagnostics stopped updating during Learn. They now remain
  current while the new diagnostic API also supports Pitch Bend.
- Legacy-editor validation rejected tiny positive sensitivities accepted by the
  native preset contract. Both paths now use the same positive/maximum bounds.

See [VERIFICATION.md](VERIFICATION.md) for passing checks and their practical
limits. NI native packet formats and physical hardware acceptance remain open.
