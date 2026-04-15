# Download UI Design

## Goal

Replace the current remote-download stderr line with a full-screen pre-play loading experience that matches the existing looper TUI, then hand off into playback as soon as enough audio is available to start. If caching continues after playback begins, show a compact in-player cache indicator instead of keeping the full loading screen on screen.

## User Experience

### Remote single tracks

- Starting a remote URL opens a dedicated loading scene instead of printing `Downloading: ...`.
- The loading scene shows:
  - track title
  - service badge
  - primary progress bar
  - downloaded amount
  - speed and ETA when available
  - a small ambient animation that matches the player palette
- The scene exits as soon as playback can safely begin, not only after the full file is downloaded.
- If the file is already cached, skip the loading scene entirely or show only a very brief branded splash.

### Remote playlists

- Each uncached track can show the same loading scene on first playback.
- Prefetched or already-cached tracks should skip or shorten the loading scene.
- Track position language should stay aligned with the player, for example `Track 2/7`.

### During playback

- If caching is still in progress after playback starts, the main player keeps a compact status badge such as `CACHE 42%` or `BUFFERING`.
- The badge should live near the existing playback status so it does not compete with the visualizer.

## Visual Direction

- Reuse the player's warm-to-cool palette and restrained borders so the loading scene feels like part of the same product.
- Make the loading composition more poster-like than the playback screen:
  - prominent title card near the top
  - service badge and state text
  - wide centered progress bar
  - quiet transfer metadata below
  - subtle animated particles or dot field for atmosphere
- Keep the main playback screen mostly intact. The goal is a polished handoff, not a full redesign of playback.

## Architecture

### State model

Add a download-oriented state model that can be rendered before playback and partially persisted into the main player:

- title
- service
- bytes downloaded
- total bytes when known
- progress fraction when known
- transfer speed
- ETA
- whether playback can start
- whether caching has completed
- current playlist position when relevant

### Data flow

- Change the `yt-dlp` download path to emit machine-readable progress data instead of waiting silently for completion.
- Parse those progress events into a shared `DownloadState`.
- Render a dedicated loading screen while the app is in pre-play mode.
- Once the start threshold is reached, transition into normal playback.
- Continue updating a reduced cache status in the main player until the background download completes.

### Handoff behavior

- The loading screen owns the terminal until playback can start.
- When playback begins, teardown should be seamless: no raw terminal flicker and no intermediate stderr logging.
- Cached tracks bypass the loading scene.

## Error Handling

- If `yt-dlp` reports private, unavailable, age-restricted, or otherwise inaccessible content, show the failure inside the loading scene before exiting.
- If total size is unknown, fall back to an indeterminate progress bar with downloaded bytes and speed.
- If the remote source stalls before startup, the loading scene should remain responsive and continue updating status instead of appearing frozen.
- Terminal restore must remain correct on success, cancellation, and failure.

## Testing

- Unit test `yt-dlp` progress parsing.
- Verify uncached remote tracks enter loading mode and then transition into playback.
- Verify cached tracks skip loading mode.
- Verify continued caching after playback updates the compact in-player badge.
- Verify playlist tracks show the correct `Track n/N` context.
- Verify remote failures render a readable loading-screen error and restore the terminal cleanly.

## Scope

### In scope

- Full-screen loading scene for remote track startup
- progress-driven handoff into playback
- compact in-player cache status
- playlist-aware loading copy

### Out of scope

- full redesign of the playback TUI
- multi-download dashboard for all prefetched tracks
- service-specific downloader rewrites
- new navigation controls
