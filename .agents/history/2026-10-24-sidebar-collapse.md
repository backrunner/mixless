# Sidebar disclosures

Local Folders and Playlists now collapse independently from their full header rows. Height clipping and the disclosure arrow share a reversible 180 ms cubic easing transition. Reduced motion settles immediately. The current playlist is retained when hidden, and MIDI browsing reveals its destination group.

Source rows use a variable-height virtual list, with stable header focus handles and scroll anchoring while rows are removed. GPUI synthesizes keyboard clicks on Enter/Space release. Key-down only suppresses the global transport shortcut, avoiding a second toggle on release.

Validation on macOS 27.0.1 (26A434), Offline audio:

- Desktop regression suite: 191 passed, 7 ignored. Final keyboard adjustment subsequently rebuilt and verified natively.
- Final dev bundle built successfully at 2026-10-24T08:33:10Z; git diff --check passed.
- Native mouse collapse, independent groups, both collapsed, restoration, rapid double-click, Enter and Space checked. Current Downloads selection and visible tracks persisted. Right-clicking DnB Set while Downloads was selected targeted DnB Set.
- Default and maximized layouts inspected. No narrow-window claim. No frame-time measurements or intermediate animation capture.
- Raw final screenshots and build metadata recorded under assets/screenshots/sidebar-collapse and assets/screenshots/capture.json. Validation used a copied database with unavailable source-file paths; expected recovery rows are visible. No live-library edits or playback.
- External-volume preview launch blocked in the self-install writable-parent probe. A temporary local bundle launched normally. Isolated preview closed after validation. No installation, commit, push or release.
