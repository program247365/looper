# Download UI Implementation Plan

## Objective

Implement a full-screen remote-download loading scene that hands off into playback as soon as startup is safe, then keeps a compact cache-progress indicator in the main player until the active track finishes caching.

## Workstreams

### 1. Progress event plumbing

- Update the `yt-dlp` download path to emit machine-readable progress output.
- Add a parser that converts those lines into structured progress events.
- Normalize:
  - bytes downloaded
  - total bytes
  - percentage
  - speed
  - ETA
  - completion state
- Preserve existing error classification for YouTube, SoundCloud, and HypeM failures.

### 2. Download state model

- Add a `DownloadState` type for remote startup.
- Include:
  - title
  - service label
  - playlist position
  - download progress fields
  - startup readiness
  - cache completion
  - human-readable status text
- Add a reduced cache-status representation for the main playback UI.

### 3. Loading-mode TUI

- Add a new loading render path in `src/tui.rs`.
- Build a dedicated loading layout with:
  - title card
  - service badge
  - primary progress bar
  - transfer metadata
  - restrained ambient animation
- Keep the visual language aligned with the existing playback palette and borders.

### 4. Handoff into playback

- Introduce an explicit pre-play mode for remote uncached tracks.
- Keep the loading scene active until the startup threshold is reached.
- Start playback as soon as the threshold is met.
- Remove raw `Downloading: ...` stderr output for the active track.
- Carry any remaining download progress into the playback header as a compact cache badge.

### 5. Playlist integration

- Reuse the loading mode for the current playlist track only.
- Use playlist context in the loading screen, for example `Track 2/7`.
- Keep prefetch of future tracks out of the full-screen loading UI.
- Optionally reflect current-track cache progress only after handoff.

### 6. Failure and restore behavior

- Render startup failures inside the loading scene where possible.
- Ensure terminal restore remains correct on:
  - successful playback start
  - remote download failure
  - cancellation
  - Ctrl-C / quit

## File-Level Changes

- `src/plugin/ytdlp.rs`
  - emit and parse `yt-dlp` progress
  - surface structured progress updates
- `src/play_loop.rs`
  - add pre-play loading mode orchestration
  - manage handoff into playback
  - update cache status while playing
- `src/tui.rs`
  - add loading scene renderer
  - add compact cache badge in playback UI
- `src/audio.rs`
  - verify playback can begin against partially downloaded local files or current startup strategy
- `tests/`
  - add parser and orchestration coverage

## Sequence

1. Add `DownloadState` and `yt-dlp` progress parsing.
2. Add the loading renderer in isolation.
3. Wire pre-play mode into remote single-track startup.
4. Add compact cache badge during playback.
5. Extend the same flow to playlist current-track startup.
6. Add tests and terminal-restore verification.

## Risks

- `yt-dlp` progress output may vary across versions.
- Total size and ETA may be unavailable for some services.
- Starting playback too early against partially downloaded files may be format-dependent.
- The loading screen must not interfere with the existing TUI event loop or terminal restore logic.

## Acceptance Criteria

- Remote uncached tracks show a full-screen loading scene instead of plain stderr logging.
- The scene transitions into playback as soon as startup is safe.
- The main player shows a compact cache-progress indicator when needed.
- Cached tracks skip the loading scene.
- Playlist startup preserves `Track n/N` context.
- Failures show readable remote-download errors without breaking terminal restore.
