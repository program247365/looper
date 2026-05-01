# CLAUDE.md

This file provides guidance to Claude Code when working with code in this repository.

## What This Is

`looper` is a Rust CLI audio looper with a `ratatui` TUI.

The main user command is still:

```bash
looper play --url <path-or-url>
```

But `--url` now accepts:

- local audio file paths
- YouTube tracks and playlists
- SoundCloud tracks and playlists
- HypeM URLs

Behavior:

- local files: play directly
- single tracks: loop forever
- playlists: play each track once, then loop the whole playlist

## Commands

```bash
make build          # debug build
make build-release  # optimized release binary
make build-macos    # x86_64 macOS release binary
make run            # play tests/fixtures/sound.mp3 on loop
make test           # non-interactive tests
make test-all       # all tests including audio-output tests
make install        # install release binary to /usr/local/bin

# Homebrew release workflow
make release        # tag current version, push, create GH release, update tap formula
make release-patch  # bump patch, then release
make release-minor  # bump minor, then release
make bump-formula   # update tap formula SHA256/version only

# Direct cargo commands
cargo build
cargo build --release
cargo test
```

## External Runtime Dependencies

Remote playback requires:

- `yt-dlp`
- `ffmpeg`

If YouTube playback fails with `403`, updating `yt-dlp` is the first thing to try.

## Install via Homebrew

```bash
brew tap program247365/tap
brew install looper
```

Tap repo: https://github.com/program247365/homebrew-tap

## Key Dependencies

- `rodio` — audio playback and sink control
- `symphonia` — duration probing, especially for VBR MP3s
- `spectrum-analyzer` — FFT and log-spaced band analysis
- `ratatui` + `crossterm` — terminal UI and input handling
- `directories` — cache directory lookup
- `serde_json` — parsing `yt-dlp` metadata output
- `stream-download` — HTTP/process-backed stream readers
- `tokio` — runtime used by streamed/process-backed audio inputs
- `structopt` — CLI parsing

## Architecture

### Main modules

- `src/main.rs` — CLI entry point, installs `color-eyre`, builds `MediaSession` + `PlaybackContext`, routes `play` to `play_loop::play_file`. On macOS, hands the main thread to `macos_runloop::run_with_tui_thread` so AppKit can dispatch media-key callbacks.
- `src/play_loop.rs` — high-level orchestration for local files, remote resolution, loading UI handoff, playlists, prefetching, and the main input/render loop. Owns the `KeyCommand` enum and the `PlaybackContext { cmd_rx, media }` struct shared with `main.rs`.
- `src/audio.rs` — `AudioPlayer`, rodio sink/output setup, decoder selection, file/HTTP/process-backed input opening, shared sample tap buffer
- `src/media_controls.rs` — cross-platform façade over `souvlaki::MediaControls`. `MediaSession::start()` returns a `(MediaSession, Receiver<KeyCommand>)`; the souvlaki callback translates `MediaControlEvent` into `KeyCommand` and forwards via the channel. `MediaSessionHandle` (cheap-cloneable, `Arc<Mutex<MediaControls>>`) exposes `set_metadata` and `set_playback` for the TUI thread to update Now Playing.
- `src/macos_runloop.rs` — macOS-only. Spawns the TUI body on a `looper-tui` worker thread and runs `NSApp.run()` on the main thread. The worker calls `std::process::exit` on completion to terminate the run loop. Activation policy is `Accessory` so looper does not appear in the Dock.
- `src/tui.rs` — playback TUI and loading TUI rendering
- `src/download.rs` — loading/progress state models and helpers for formatting bytes/speed/ETA
- `src/plugin/` — remote service resolution and `yt-dlp` integration
- `src/playback_input.rs` — playback input abstraction (`File`, `HttpStream`, `ProcessStdout`) plus pending-download metadata

### Remote playback model

The project now uses a hybrid remote architecture.

- `src/plugin/mod.rs`
  - plugin registry
  - cache directory lookup via `ProjectDirs`
  - dispatch to YouTube, SoundCloud, HypeM, or generic `yt-dlp`
- `src/plugin/ytdlp.rs`
  - checks `yt-dlp` availability
  - extracts metadata and playlist entries
  - downloads/caches tracks
  - emits machine-readable progress for the loading TUI
  - contains current service-specific failure explanations
- `src/plugin/youtube.rs`
  - normalizes some watch URLs with both `v=` and `list=`
  - currently uses the more reliable download-first path
- `src/plugin/soundcloud.rs`
  - prefers stream-first resolution and falls back to download-first
- `src/plugin/hypem.rs`
  - prefers stream-first resolution and falls back to download-first

### Playback inputs

`PlaybackInput` currently supports:

- `File(PathBuf)` — local files and cached remote tracks
- `HttpStream { .. }` — direct HTTP-backed stream reader
- `ProcessStdout { .. }` — process-backed stream through `stream-download`

Note that YouTube is intentionally on the cached-file path right now because direct/process streaming proved less reliable than download-first with current `yt-dlp` behavior.

### TUI states

There are now two major UI modes:

- loading scene for uncached remote startup
  - title
  - service label
  - progress bar
  - downloaded bytes / total bytes
  - speed / ETA
  - ambient animation
- playback scene
  - header with service badge (`YT`, `SC`, `HM`)
  - scatter visualizer
  - progress bar
  - footer / micro-status
  - optional compact cache badge like `CACHE 42%`

### Playlist behavior

- single local or remote track: `repeat_infinite()`
- playlist: play each track once, then loop the playlist
- `PrefetchWorker` in `play_loop.rs` uses a bounded channel and background thread to cache current/next tracks where applicable
- remote playlists are re-resolved each full loop so expiring service URLs are less likely to be reused forever

### Threading model

- rodio owns the audio output thread
- on Linux/Windows the main thread owns the TUI event loop and app state
- on **macOS** the main thread is owned by `NSApp.run()`; the TUI event loop runs on a `looper-tui` worker thread (required for `MPRemoteCommandCenter` callbacks). `std::process::exit` from the worker terminates the runloop on completion.
- the visualizer reads from `sample_buf: Arc<Mutex<VecDeque<f32>>>`
- prefetch uses a background worker thread
- some stream-backed audio inputs create a Tokio runtime inside `AudioPlayer`
- media-key events from `souvlaki` arrive on the OS-specific thread (macOS: main / AppKit; Linux: souvlaki's own DBus thread) and are forwarded to the TUI thread via an `mpsc::Receiver<KeyCommand>` drained inside `run_loop`

## Notable Design Decisions

- `reattach_stdin_to_tty()` (Unix only) reopens `/dev/tty` when stdin is piped so crossterm still works
- loop counting for repeated single tracks uses wall-clock elapsed time because `repeat_infinite()` does not reset sink position
- per-band AGC keeps the scatter visualizer lively across different mixes
- YouTube currently favors reliability over immediacy: cached download-first instead of direct stream-first
- remote loading is presented in-TUI instead of as plain stderr logging
- `souvlaki` is wired with the `use_zbus` feature so Linux builds don't need `libdbus-1-dev`; macOS uses `MPRemoteCommandCenter` + `MPNowPlayingInfoCenter` directly. Windows is intentionally unwired (would need a hidden message-only HWND + a per-tick `pump_event_queue`).

## Tests

- integration tests live in `tests/integration.rs`
- `test_play` remains ignored because it needs real audio output and a terminal
- `src/plugin/ytdlp.rs` has parser tests for `yt-dlp` progress lines

When changing remote playback:

- run `cargo build`
- run `cargo test`
- prefer a real manual smoke test with a public URL for the affected service

## Things Likely To Need Care

- `yt-dlp` output formats and YouTube behavior can change over time
- service-specific restrictions can break public URLs without any Rust-side regression
- terminal restore correctness matters whenever touching loading/playback state transitions
- the worktree may contain user changes; do not revert unrelated modifications
