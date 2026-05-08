# Looper

> A CLI audio looper with a real-time FFT visualizer, startup screen, default history browser, favorites, fullscreen mode, and online URL support.

![Looper fullscreen visualizer](screenshots/looper.png)

## What It Does

`looper` plays audio in a terminal UI built with `ratatui`.

It supports:

- local audio files
- YouTube URLs
- SoundCloud URLs
- HypeM URLs
- single tracks and playlists
- infinite looping for single tracks
- whole-playlist looping for playlists
- pause / resume
- fullscreen visualizer
- centered ASCII startup/loading screen with cheeky boot logs
- default no-arg startup into playlist history
- SQLite-backed playback history and favorites
- remote download/loading UI with progress, speed, and ETA
- small source badges in the TUI for supported services (`YT`, `SC`, `HM`)
- animated terminal/tab title with playback, pause, and loading status

## Install

### Homebrew (Apple Silicon)

Fresh install:

```shell
brew tap program247365/tap
brew install looper
```

Upgrade an existing install:

```shell
brew update
brew upgrade program247365/tap/looper
```

The Homebrew formula ships a prebuilt binary for `aarch64-apple-darwin`, so install and upgrade are a small download and a file move — no compile step on your machine. `ffmpeg` and `yt-dlp` are pulled in automatically as runtime dependencies.

Intel macOS users: there is no prebuilt binary. See "Build from source" below or use `brew install --HEAD program247365/tap/looper` to compile from `main`.

### Build from source

```shell
git clone https://github.com/program247365/looper.git
cd looper
make install
```

Requires Rust. Install via [rustup](https://rustup.rs) if needed.

For remote URL playback (YouTube, SoundCloud, HypeM), also install `yt-dlp` and `ffmpeg`:

```shell
brew install yt-dlp ffmpeg
```

If YouTube playback starts failing with `403` errors, update `yt-dlp` first.

## Usage

### Default startup

```shell
looper
```

This opens the playlist history browser with no active playback. Press `Enter` on a row to start playing it.

If you want to skip the browser and jump straight into playback, use `looper play --url ...`.

### Local file

```shell
looper play --url "/path/to/your/song.mp3"
```

### YouTube

```shell
looper play --url "https://www.youtube.com/watch?v=xAR6N9N8e6U"
```

### SoundCloud

```shell
looper play --url "https://soundcloud.com/odesza/line-of-sight-feat-wynne-mansionair"
```

### HypeM

```shell
looper play --url "https://hypem.com/track/2gq0d/CHVRCHES+-+Clearest+Blue"
```

### Playlists

```shell
looper play --url "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI"
```

## How Remote Playback Works

- startup opens the local SQLite database, runs embedded migrations, and then begins loading playback
- `yt-dlp` extracts track metadata and media URLs
- remote audio is cached locally (see [Data and Cache Locations](#data-and-cache-locations))
- uncached remote tracks show a full-screen loading scene before playback
- single tracks loop forever
- playlists play each track once, then loop the entire playlist
- background prefetch caches upcoming playlist tracks when possible

Current behavior is intentionally pragmatic:

- YouTube uses a download-first cached path for reliability
- SoundCloud and HypeM prefer a stream-first path and fall back to cached download when needed

## Data and Cache Locations

Remote tracks are cached locally after download:

| Platform | Cache directory |
|----------|----------------|
| macOS | `~/Library/Caches/sh.kbr.looper/` |
| Linux | `~/.cache/looper/` |

Playback history and favorites live in a SQLite database (`looper.sqlite3`). Where it lives depends on your sync setup — see [Cross-Device Sync](#cross-device-sync) below.

- startup applies pending embedded migrations automatically — no manual steps needed when upgrading
- bare `looper` loads this history first and lets you replay from it
- history is tracked per playable URL or canonical local file path
- each track stores title, platform, favorite state, last played timestamp, play count, cumulative time played, and which computer played it last

## Cross-Device Sync

looper always reads and writes a fast local copy of `looper.sqlite3`. If you point it at a cloud folder (iCloud Drive, Dropbox, anything that syncs files), looper will pull from that folder at startup and push to it on quit. The cloud folder never holds the live database — it's just a passive copy that the cloud provider replicates between your machines on its own.

This avoids the long-standing footguns of running SQLite directly on a cloud-synced filesystem (corrupted WAL/SHM sidecars, surprise permission denials, evicted files).

### Where the live database lives

| Platform | Live database path |
|----------|--------------------|
| macOS | `~/Library/Application Support/sh.kbr.looper/looper.sqlite3` |
| Linux | `~/.local/share/looper/looper.sqlite3` |

By default no replication runs. Looper just uses the local path above.

### Replicate via a cloud folder

Point looper at any folder your cloud provider keeps in sync:

```shell
looper config set sync-folder "$HOME/Library/Mobile Documents/com~apple~CloudDocs/looper"
# Replication folder set to: ...
# looper will pull from this folder at startup and push to it on quit.
# The live DB stays at the platform data directory.
```

Run this once on each computer that should share history. The cloud provider takes care of moving `looper.sqlite3` between machines in the background.

**Verify it's working:**

```shell
looper config show
# sync_folder = /Users/you/Library/Mobile Documents/.../looper (replicated on startup/quit)

ls "$HOME/Library/Mobile Documents/com~apple~CloudDocs/looper/"
# looper.sqlite3
```

### macOS: iCloud Drive needs Files-and-Folders permission

The first time looper tries to read or write inside `~/Library/Mobile Documents/...`, macOS will silently deny access until you grant the terminal app that launches looper permission. Open **System Settings → Privacy & Security → Files and Folders** (or **Full Disk Access**) and toggle on **iCloud Drive** for your terminal (Terminal, iTerm, Ghostty, etc.). Restart the terminal so the new entitlement takes effect.

If permission isn't granted, looper still runs against the local DB, surfaces a `History sync disabled` banner at startup, and prints a one-line warning to stderr on quit. Nothing crashes.

### Sync semantics: last-quitter wins

Replication is a file copy in both directions:

- **At startup**: if the cloud copy has a more recent `MAX(last_played_at)` than the local copy, looper replaces local with the cloud copy.
- **At quit**: looper checkpoints the WAL, then atomically replaces the cloud copy with the local one.

This is enough for one-human-at-a-time use across multiple Macs (typical single-user setup). It is **not** a general-purpose multi-master merge: if you play on two machines simultaneously, whichever quits last overwrites the other's session, and your cloud provider may produce conflict copies (e.g. `looper.sqlite3 conflicted-copy 2`). Resolve by closing one, picking the version you want to keep, and deleting the rest.

### Disable replication

```shell
rm "$HOME/.config/looper/sync_folder"
```

Or just don't set it. Looper falls back to local-only without complaint.

## Keys

| Key | Action |
|-----|--------|
| `Enter` | Replay the selected track from the default history browser |
| `Space` | Pause / Resume |
| `f` | Toggle fullscreen visualizer |
| `s` | Toggle favorite for the currently playing track |
| `p` | Toggle the played-songs panel |
| `Cmd-P` | Attempt to toggle the played-songs panel when the terminal forwards the modifier |
| `q` / `Ctrl-C` | Quit |

### Played-Songs Panel

Bare `looper` opens directly into playlist history. During playback, the played-songs panel is hidden by default and opens over the minimal UI.

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up |
| `h` / `l` | Change sort field |
| `r` | Reverse sort direction |
| `s` | Toggle favorite for the selected row |
| `Enter` | Replay the selected track |
| `p` / `Esc` | Close the panel |

Sort fields:

- time played
- last played
- platform
- title
- times played

## Development

```shell
make run           # play fixture file (tests/fixtures/sound.mp3)
make test          # run tests
make build         # debug build
make build-release # optimized release binary
```

Useful direct commands:

```shell
cargo build
cargo build --release
cargo test
```

## Notes

- Public online URLs work best. Private, age-restricted, or members-only content may still fail depending on `yt-dlp` access.
- If a YouTube watch URL includes both `v=` and `list=`, `looper` currently normalizes it toward single-video playback unless you use the playlist URL directly.
- The remote loading UI is designed to hand off into playback cleanly rather than waiting on a full silent download.
- The startup screen and loading copy are intentionally a little cheeky.

## Releasing

```shell
make release-patch    # bump patch version (0.5.x → 0.5.x+1) and release
make release-minor    # bump minor version (0.5.x → 0.6.0) and release
make smoke-test       # (optional) verify the published formula installs cleanly
```

`make release-patch` / `make release-minor` runs end-to-end:

1. Bumps the version in `Cargo.toml` and commits it
2. Tags `v<version>` and pushes the tag
3. The `Release` GitHub Actions workflow (`.github/workflows/release.yml`) fires on the tag, builds an `aarch64-apple-darwin` binary on a `macos-14` runner, and attaches it to the GitHub release
4. `make bump-formula` (auto-invoked) polls the release, computes the SHA256, regenerates the Homebrew formula via `scripts/render-formula.sh`, and pushes the update to [`program247365/homebrew-tap`](https://github.com/program247365/homebrew-tap)

Total wall-clock time is typically 3–4 minutes (most of it the arm64 cargo build on CI).

`make smoke-test` then reinstalls the formula on your machine and asserts:

- the formula uses the prebuilt-binary install path (`bin.install "looper"`)
- the tap version matches `Cargo.toml`
- `looper --help` runs successfully

If you need to recover from a partial release (e.g. CI flaked between tag push and formula update), re-run `make bump-formula` directly — it is idempotent and will wait for the asset, then push to the tap.
