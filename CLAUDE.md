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

# Run a specific test
cargo test test_help

# Direct cargo run
cargo run -- play --url tests/fixtures/sound.mp3
```

## Architecture

Four source files:

- `src/main.rs` — CLI entry point using `structopt`. Declares all modules, routes `Command::Play` to `play_loop::play_file()`.
- `src/audio.rs` — `AudioPlayer` struct wrapping `rodio`. Owns `OutputStream` (must stay alive as a field or audio stops), `Sink`, and probed `duration`. Exposes `pause()`, `resume()`, `is_paused()`.
- `src/tui.rs` — `AppState` struct, terminal setup/restore helpers, and `draw()` which renders the ratatui layout: now-playing header, progress bar, animated visualizer, keybindings footer.
- `src/play_loop.rs` — Orchestrator. Creates `AudioPlayer` and `AppState`, sets the panic hook for terminal restore, runs the 100ms-tick crossterm event loop (Space = pause/resume, q = quit), tracks loop count via elapsed time vs. duration.

Audio runs on rodio's internal thread; the main thread owns the ratatui event loop and mutates `AppState`. No shared mutable state across threads.

## Tests

Integration tests live in `tests/integration.rs` using `assert_cmd`. The `test_play` test is `#[ignore]` because it requires actual audio output and a terminal. The fixture audio file is at `tests/fixtures/sound.mp3`.
