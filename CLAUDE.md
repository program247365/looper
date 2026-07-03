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
- Spotify tracks, playlists, and albums (`https://open.spotify.com/...` or
  `spotify:...`), via librespot — **Spotify Premium required**

Behavior:

- local files: play directly
- single tracks: loop forever
- playlists: play each track once, then loop the whole playlist

Spotify needs a one-time `looper spotify login` (OAuth browser flow, credentials
cached). See "Spotify playback model" below.

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

YouTube / SoundCloud / HypeM playback requires:

- `yt-dlp`
- `ffmpeg`

If YouTube playback fails with `403`, updating `yt-dlp` is the first thing to try.

Spotify requires **neither** `yt-dlp` nor `ffmpeg` (librespot decodes in-process)
— only a Spotify Premium account and a one-time `looper spotify login`.

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
- `librespot-core` / `-playback` / `-metadata` / `-oauth` — Spotify session,
  in-process decode, metadata, and OAuth login. `librespot-playback` is built
  with `default-features = false` so its bundled (older) rodio backend stays out
  of the tree — looper feeds a custom `Sink` instead. **`vergen` is pinned to
  9.0.6 in `Cargo.lock`**; a `cargo update` that pulls vergen 9.1.0 breaks
  librespot-core's build script (vergen-lib trait mismatch) — re-pin with
  `cargo update -p vergen --precise 9.0.6` (also noted in `Cargo.toml`/`Makefile`).

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
- `src/spotify/` — Spotify via librespot: shared session, OAuth login, metadata
  resolution, the librespot-`Sink`→rodio-`Source` bridge. `main.rs` routes the
  `spotify login` subcommand here. See "Spotify playback model" below.
- `src/playback_input.rs` — playback input abstraction (`File`, `HttpStream`, `ProcessStdout`, `Spotify`) plus pending-download metadata

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

`plugin::resolve_url` intercepts Spotify URLs/URIs **before** the `yt-dlp`
availability check and dispatches to `crate::spotify::resolve`, so Spotify works
without `yt-dlp` installed.

### Spotify playback model

Spotify is not a `yt-dlp` plugin — it has no downloadable audio. `src/spotify/`
uses librespot:

- `src/spotify/mod.rs`
  - `is_spotify_url`, URL/URI parsing (`open.spotify.com/...`, `intl-xx`
    prefixes, `spotify:` URIs)
  - a shared runtime (`OnceLock`, never replaced — it hosts player tasks) plus a
    rebuildable `Session` (`Mutex<Option<Session>>`). `session()` reconnects from
    cached credentials when the previous session `is_invalid()` (sleep/wake,
    network change) — librespot's core `Session` does **not** auto-reconnect, so
    track transitions self-heal here. `login()` runs the `librespot-oauth`
    browser flow once. `Session::new` calls `Handle::current()`, so it must be
    built **inside** the runtime (`runtime.block_on`)
  - `resolve()` → `Vec<TrackInfo>` for a track, playlist, or album, fetching
    track metadata + album art concurrently (bounded batches). Album art is a
    public `i.scdn.co` JPEG keyed by file id, cached under `spotify/art/`
  - `ensure_track_available()` uses librespot's `AudioItem` availability to fail
    a single unplayable track at resolve time, so `resolve_url_with_startup`
    surfaces the "track unavailable" modal instead of playing silence
- `src/spotify/search.rs` — catalog search via the public Web API (`/v1/search`),
  authorized with a bearer token minted from the librespot session
  (`session.token_provider().get_token(...)` — comma-separated scopes string).
  Returns `SearchResults { tracks, albums, playlists }` of `SearchItem`s whose
  `uri` is a valid `resolve()` target. Playlist `items` can contain literal
  `null`s (post-2024 API changes) — the parser filters them.
- `src/spotify/sink.rs` — the bridge. librespot's `Player` pushes decoded PCM
  into a custom `Sink`; a bounded channel carries it to a rodio `Source`. The
  sink blocks under backpressure (throttling the decoder to real time); the
  source yields silence on underrun. An `EndSignal` lets the source end on
  demand: single tracks loop forever (listener re-`load`s on `EndOfTrack`),
  playlist tracks finish so `play_loop`'s `sink.empty()` advances to the next.

### Playback inputs

`PlaybackInput` currently supports:

- `File(PathBuf)` — local files and cached remote tracks
- `HttpStream { .. }` — direct HTTP-backed stream reader
- `ProcessStdout { .. }` — process-backed stream through `stream-download`
- `Spotify { track_uri }` — handled in `AudioPlayer::new` by the librespot
  bridge (`src/spotify/`); never reaches the file/stream reader path. Pause works
  via rodio backpressure; seek is a no-op

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
- history browser (`browse_history_session`) — the landing screen when `looper`
  is run with no `--url`; also reachable mid-playback via the `p` overlay. `enter`
  replays the selected row.
- "track unavailable" modal (`draw_replay_error`) — non-fatal overlay shown when
  a replay target can't be resolved; `d` prunes the dead row, any other key
  returns to the history browser.
- Spotify search overlay (`/` from playback or the history browser;
  `draw_search_overlay`). Query focus: type, Enter searches (one blocking Web
  API call; a "searching…" frame is drawn first), Esc closes. Results focus:
  `j`/`k` move (headers are skipped), `gg`/`G` jump, `/` re-edits the query,
  Enter plays the selection through the normal replay rail
  (`LoopAction::ReplayTarget` in playback; `play_file_session` in the history
  browser). While open the overlay captures all keys; only Ctrl-C quits.

### Playlist behavior

- single local or remote track: `repeat_infinite()`
- playlist: play each track once, then loop the playlist
- `PrefetchWorker` in `play_loop.rs` uses a bounded channel and background thread to cache current/next tracks where applicable. **Spotify tracks are skipped** by the prefetcher (they stream in-process via librespot and have no `source_url` to download), so there is a brief loading screen between Spotify playlist tracks
- remote playlists are re-resolved each full loop so expiring service URLs are less likely to be reused forever

### Threading model

- rodio owns the audio output thread
- on Linux/Windows the main thread owns the TUI event loop and app state
- on **macOS** the main thread is owned by `NSApp.run()`; the TUI event loop runs on a `looper-tui` worker thread (required for `MPRemoteCommandCenter` callbacks). `std::process::exit` from the worker terminates the runloop on completion.
- the visualizer reads from `sample_buf: Arc<Mutex<VecDeque<f32>>>`
- prefetch uses a background worker thread
- some stream-backed audio inputs create a Tokio runtime inside `AudioPlayer`
- Spotify owns a process-wide Tokio runtime in `src/spotify/` (`OnceLock`, never replaced — librespot's `Player` and the end-of-track loop listener run on it). The `Session` lives in a `Mutex<Option<Session>>` and is rebuilt by `session()` when `is_invalid()`, so a dropped connection reconnects at the next track (the currently-playing track's `Player` is bound to the old session and can't self-heal mid-stream; a single track looping forever won't recover until restart). The bridge's end-of-track listener is aborted when the `AudioPlayer`'s `SpotifyPlayback` drops, releasing the `Player`
- media-key events from `souvlaki` arrive on the OS-specific thread (macOS: main / AppKit; Linux: souvlaki's own DBus thread) and are forwarded to the TUI thread via an `mpsc::Receiver<KeyCommand>` drained inside `run_loop`

## Notable Design Decisions

- `reattach_stdin_to_tty()` (Unix only) reopens `/dev/tty` when stdin is piped so crossterm still works
- loop counting for repeated single tracks uses wall-clock elapsed time because `repeat_infinite()` does not reset sink position
- per-band AGC keeps the scatter visualizer lively across different mixes
- YouTube currently favors reliability over immediacy: cached download-first instead of direct stream-first
- remote loading is presented in-TUI instead of as plain stderr logging
- an unresolvable replay target (private/removed/region-locked/expired live
  stream) is **not** fatal: `resolve_url_with_startup` returns
  `ResolveStartupOutcome::Failed`, `play_file_session` shows the "track
  unavailable" modal, and replay from the history browser returns to the list
  (`SessionOutcome::BackToHistory`) rather than exiting. Quitting playback with
  `q` still exits the app (`SessionOutcome::Quit`). This is intentional — a
  "jukebox historian" accumulates links that inevitably rot.
- Spotify playback uses librespot (reverse-engineered Spotify Connect),
  **Premium-only**, with a custom librespot `Sink` feeding rodio so the
  visualizer keeps working — rather than librespot's own rodio backend (which
  would bypass the sample tap and also drag a second, incompatible rodio into
  the tree). `librespot-playback` is therefore `default-features = false`.
- `vergen` is pinned to `9.0.6` (`Cargo.lock`) to keep librespot-core's build
  script compiling; see the note in `Cargo.toml`/`Makefile`. Re-pin after a
  `cargo update` with `cargo update -p vergen --precise 9.0.6` if the build
  fails with a vergen-lib trait mismatch.
- a directly-requested **single** Spotify track that is unavailable is caught at
  resolve (`ensure_track_available`) so it shows the modal; unavailable tracks
  inside a playlist/album are silently dropped during concurrent metadata fetch.
- the OS Now Playing widget (`media_controls::set_metadata`) gets title, artist
  (real artist when the source provides one — Spotify `Track.artists`, yt-dlp
  `artist`/`uploader`; falls back to the service name), album (the playlist/album
  `collection`), and cover art. Cover art is passed as a percent-encoded
  `file://` URL of `thumbnail_path` (souvlaki hands it to `NSImage` /
  MPRIS). Stream-first SoundCloud/HypeM tracks fetch their thumbnail lazily at
  playback via `ytdlp::fetch_thumbnail`; local files (no embedded art) use a
  bundled fallback cover embedded from `assets/local-cover.png` via
  `include_bytes!` and materialized into the cache, so the widget is never blank.
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
