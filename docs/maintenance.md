# Maintenance watch list

Things that need periodic re-checking: upstream issues we're waiting on,
pinned versions, and warnings that will eventually become errors. Check these
when bumping the toolchain or doing a dependency pass. Remove entries once
resolved.

## `block v0.1.6` future-incompat warning (via souvlaki/cocoa)

**Symptom:** every `cargo build` prints
`warning: the following packages contain code that will be rejected by a
future version of Rust: block v0.1.6`.

**Cause:** `block 0.1.6` (old Objective-C blocks bindings) contains a static
of an uninhabited type, which rustc will eventually reject
([rust-lang/rust#74840](https://github.com/rust-lang/rust/issues/74840)).
It reaches us two ways (`cargo tree -i block`):

1. our own `cocoa` + `objc` deps — used only in `src/macos_runloop.rs`
2. `souvlaki` → `cocoa` + `block` directly

**Why we haven't fixed it (checked 2026-08-04):** souvlaki 0.8.3 is the
latest release and its master still depends on `block`/`cocoa`/`objc` — no
objc2 migration upstream, no open issue or PR. Migrating only our
`macos_runloop.rs` to `objc2-app-kit` would not silence the warning.

**When it becomes a hard error:** bump souvlaki if a fixed release exists;
otherwise point at a fork migrated to `objc2`, and migrate
`src/macos_runloop.rs` off `cocoa` at the same time.

**Re-check:** does a souvlaki release > 0.8.3 drop `block`/`cocoa`?
`cargo info souvlaki` and https://github.com/Sinono3/souvlaki

## `vergen` pinned to 9.0.6

`cargo update` pulling vergen 9.1.0 breaks librespot-core's build script
(vergen-lib trait mismatch). Re-pin with
`cargo update -p vergen --precise 9.0.6`. Also noted in `Cargo.toml` and the
`Makefile`.

**Re-check:** after any librespot upgrade, try unpinning — a librespot
release built against newer vergen makes the pin obsolete.
