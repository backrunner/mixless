# Security policy

## Supported versions

mixless is pre-1.0. Only the latest commit on `main` receives fixes; there are
no maintained release branches yet.

## Reporting a vulnerability

Please do not open public issues or pull requests for security
vulnerabilities. Email **dev@backrunner.top** with:

- a description of the issue and its impact,
- steps to reproduce, and the affected commit or build,
- any suggested remediation, if you have one.

You will receive an acknowledgement and, where possible, a heads-up before a
fix lands. Thank you for helping keep mixless and its users safe.

## Scope notes

- mixless executes locally installed `yt-dlp` and `FFmpeg` binaries when a user
  explicitly acquires audio, and loads Spotify credentials the user provides.
  Reports about credential handling, downloaded model integrity, and malicious
  audio files are in scope; vulnerabilities in the external tools themselves
  should be reported upstream.
- Model artifacts are fetched over HTTPS from pinned upstream sources and
  verified before use; see `.agents/reference/native-inference.md`.
