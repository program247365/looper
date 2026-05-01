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

### Homebrew

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

### Build from source

```shell
git clone https://github.com/program247365/looper.git
cd looper
make install
```

Requires Rust. Install via [rustup](https://rustup.rs) if needed.

### External tools for online playback

Remote URL playback depends on:

- `yt-dlp`
- `ffmpeg`

Install them with Homebrew:

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

looper keeps history in sync across your computers by storing `looper.sqlite3` in a shared folder (iCloud Drive, Dropbox, or any folder you choose). No account, no server — just a file in a folder you already sync.

### How it picks where to store the database

On every launch, looper resolves the database location in this order:

1. **Configured sync folder** — if you've run `looper config set sync-folder`, that path wins
2. **iCloud Drive (macOS only)** — if iCloud Drive is active, looper automatically uses `~/Library/Mobile Documents/com~apple~CloudDocs/looper/looper.sqlite3`
3. **Platform default** — fallback when neither of the above applies

| Platform | Default database path |
|----------|-----------------------|
| macOS (no iCloud) | `~/Library/Application Support/sh.kbr.looper/looper.sqlite3` |
| Linux | `~/.local/share/looper/looper.sqlite3` |

### iCloud Drive (zero config on macOS)

If you use iCloud Drive, nothing extra is needed. On first launch after installing (or upgrading to) this version, looper:

1. Detects iCloud Drive is active
2. Creates `~/Library/Mobile Documents/com~apple~CloudDocs/looper/looper.sqlite3`
3. Merges any existing local history into the iCloud database automatically
4. Archives the old local database to `.sqlite3.bak` so it's never run twice

Once the iCloud file syncs to your other Macs (usually within a minute), every machine shares the same history. No commands needed.

**Verify it's working:**

```shell
looper config show
# sync_folder = (auto — iCloud Drive if available, otherwise platform default)

ls ~/Library/Mobile\ Documents/com~apple~CloudDocs/looper/
# looper.sqlite3
```

### Dropbox, OneDrive, or any synced folder

Point looper at any folder your cloud provider keeps in sync:

```shell
looper config set sync-folder ~/Dropbox/looper
# Sync folder set to: /Users/you/Dropbox/looper
# looper will use this folder for looper.sqlite3 on next launch.
```

Run this once on each computer. On the next launch, looper moves (with merge) to that folder.

**Verify it's working:**

```shell
looper config show
# sync_folder = /Users/you/Dropbox/looper

ls ~/Dropbox/looper/
# looper.sqlite3
```

### Check current config at any time

```shell
looper config show
```

### Two-computer upgrade scenario

If both computers already have local history from an older version of looper:

1. **Computer A** upgrades → detects iCloud (or configured folder) → merges its old local history in → archives old file
2. iCloud syncs to Computer B (the database now has A's history)
3. **Computer B** upgrades → opens the iCloud file (already has A's data) → merges its own old local history in

Result: the shared database has all plays from both computers, tagged with the machine that played each track. No data is lost.

### Merge rules (when two histories combine)

| Field | Result |
|-------|--------|
| Play count | Sum of both |
| Time played | Sum of both |
| First played | Earliest of both |
| Last played | Latest of both |
| Favorite | `true` if either copy is favorited |
| Last played on | Computer with the more recent play |

### A note on concurrent access

looper enables WAL mode on the database, which makes it safe for one machine to read while another writes. Playing on two machines simultaneously and writing to the same file is unusual and could cause conflicts — for typical single-user use (one active machine at a time) this is not an issue.

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
make release-patch
make release-minor
```
