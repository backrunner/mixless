# Rust compatibility patches

These two crates are pinned copies of the published registry packages and are
selected with `[patch.crates-io]`. `UPSTREAM.json` retains each original package
checksum. Original authorship, license metadata and upstream source are retained.
The patches address Rust 1.99 future-incompatibility diagnostics without changing
dependency APIs or increasing mixless's Rust 1.88 minimum.

- [`block` 0.1.6](https://github.com/SSheldon/rust-block), MIT: declare the foreign
  `_NSConcreteStackBlock` symbol with an inhabited byte type and obtain its
  address using `addr_of!`. The block's `isa` field still holds the same symbol
  address as an opaque class pointer. No byte is read and no reference to an
  uninhabited value is constructed. Foreign declarations and callback types
  explicitly name the C ABI, preserving the previous implicit ABI.
- [`proc-macro-error2` 2.0.1](https://github.com/GnomedDev/proc-macro-error-2),
  MIT selected from MIT/Apache-2.0: make `extern crate proc_macro` public, matching
  its existing public re-export.

Published development-only test harness dependencies are omitted from these
copies; they are not workspace members. The application build and native
runtime checks exercise the patches. The standalone MIT notices are also copied
into the app bundle by mixless-tools.

Remove these overrides when compatible upstream releases become available.
Do not edit Cargo's shared registry cache to apply these changes.
