# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What This Is

`looper` is a Rust CLI that plays a single audio file on an infinite loop with a ratatui TUI. The entire useful surface is one command: `looper play --url <path>`.

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
make release-patch  # bump patch (0.1.0 → 0.1.1), then release
make release-minor  # bump minor (0.1.x → 0.2.0), then release
make bump-formula   # update tap formula SHA256/version only (after manual tag)

# Run a specific test
cargo test test_help
```

## Install via Homebrew

```bash
brew tap program247365/tap
brew install looper
```

Tap repo: https://github.com/program247365/homebrew-tap

## Key Dependencies

- **rodio** — audio playback (owns the audio output thread)
- **symphonia** — duration probing for VBR MP3s (Xing/VBRI headers) when rodio's decoder returns `None`
- **spectrum-analyzer** — FFT + frequency spectrum from raw samples
- **ratatui** + **crossterm** — TUI rendering and terminal event handling
- **structopt** — CLI argument parsing

## Architecture

Four source files:

- `src/main.rs` — CLI entry point using `structopt`. Declares all modules, routes `Command::Play` to `play_loop::play_file()`.
- `src/audio.rs` — `AudioPlayer` wrapping rodio. Owns `OutputStream` + `Sink`, probes duration (rodio fallback → symphonia Xing/VBRI header), exposes shared `sample_buf` ring buffer for FFT. `SampleTap<S>` intercepts samples on the audio thread via `try_lock`.
- `src/tui.rs` — `AppState`, terminal setup/restore, `draw()`. Scatter visualizer uses per-cell deterministic hash for stable dot placement; `f` toggles fullscreen. Progress bar renders `━━●─── 0:42/3:12` inline.
- `src/play_loop.rs` — Orchestrator. 30ms-tick crossterm event loop (Space = pause, `f` = fullscreen, q = quit). Calls `update_visualizer()` each tick: reads ring buffer, down-mixes stereo, Hann window, FFT via `spectrum-analyzer`, maps to 32 log-spaced bands with asymmetric smoothing.

### Threading model

Audio runs on rodio's internal thread; main thread owns the event loop and `AppState`. The only shared state is `sample_buf: Arc<Mutex<VecDeque<f32>>>` — a ring buffer of raw f32 samples. `SampleTap` writes to it from the audio thread using `try_lock` (never blocks playback). The main thread reads it each tick to compute FFT bands.

### Notable design decisions

- **`reattach_stdin_to_tty()`** (Unix only): reopens `/dev/tty` when stdin is piped, so crossterm key events work even when launched from scripts or pipes.
- **Loop counting via wall clock**: `repeat_infinite()` doesn't reset `Sink::get_pos()`, so elapsed time is tracked manually with `Instant` and reset when it exceeds the probed duration.
- **Per-band AGC**: each of the 32 frequency bands tracks its own peak with slow decay (0.998), normalizing against it so quiet bands still produce visible movement.

## Tests

Integration tests live in `tests/integration.rs` using `assert_cmd`. The `test_play` test is `#[ignore]` because it requires actual audio output and a terminal. The fixture audio file is at `tests/fixtures/sound.mp3`.
