# AutoMix order mode belongs to its source playlist

The reported failure was reproduced before the fix: the native-state regression
`state::library_refresh::tests::order_modes_belong_to_the_source_playlist`
failed with `new source inherited the previous source's BEST` after selecting a
different playlist. `best`, `chosen` and the shuffle atomic described the previous
source without resetting when the active source identity changed.

Explicit ORDER, SHUFFLE and BEST selections are now remembered by playlist ID
for the lifetime of the UI state, with All Tracks as its own source. Returning
to a source restores its choice. A source without an explicit choice resolves
its own Best cache, falling back to ORDER while an uncached result warms in the
background. Same-source refreshes and edits do not reset the choice.

Source selection cancels the previous ordering request immediately, even when
the two playlists contain identical track IDs. The prepared queue is cleared
while new rows load; the engine's committed handoff is retained. Existing safe
queue replacement handles the new source when its rows arrive. Late old workers
cannot reselect their old source. Under Keep current, browsing another playlist
does not change the active source or its mode.

Validation:

- Reproduced the original mode leak with a failing GPUI regression before edits.
- Source selection regressions: 5 passed, including manual BEST/ORDER restoration,
  persisted cache lookup after leaving a manual ORDER source, independent All
  Tracks/SHUFFLE choices and Keep current isolation.
- Full desktop suite: 196 passed, 7 ignored.
- Final workspace suite excluding desktop: 619 passed, 28 ignored. An earlier
  run encountered three failures in separately changing melody/stem planning;
  those were resolved before the final successful run.
- Development app bundle refreshed with `./dev.sh --build`.
- `git diff --check` passed.

This change does not persist manual order choices across application restarts.
No live playback session was restarted and no new listening evidence was claimed.
Existing unrelated working-tree changes were retained.

## Missing-cache fallback when restoring BEST

A further regression reproduced a remembered-BEST bypass: returning to a source
whose playlist order had changed immediately restored the BEST flag even though
that input permutation had no valid cached result. The new native-state regression
failed before the fix with `remembered BEST must wait for a valid cache`.

Restoring a source now starts in ORDER and resolves its cache, including when
that source explicitly preferred BEST. Only a valid in-memory or persisted hit
activates BEST. A miss releases cache lookup before background optimization;
the remembered preference remains available for the next source selection/start,
but completion and same-source refreshes leave this pass in ORDER.

Order selection/cache activation now lives in `preparation/selection.rs`. Regression
coverage includes both a cached-BEST source switching to a new uncached three-track
source and a remembered-BEST source with a changed playlist. Both tests wait for a
real persisted result and verify that the current mode and playback queue remain
ORDER afterward. Cached BEST restoration, explicit ORDER/SHUFFLE and Keep current
regressions remain covered.

Final desktop suite for this refinement: 198 passed, 7 ignored. The development
bundle was rebuilt successfully without restarting live playback. Final workspace suite excluding desktop, run serially: 621 passed, 28 ignored.
The first concurrent run hit two native HID inventory timeouts; both passed in
the complete serial rerun. No musical scoring or DSP code changed
for this refinement.
