# Loading Screen Polish Design

## Context

`looper play --url ...` can spend noticeable time resolving long YouTube videos before playback starts. The existing startup renderer shows the logo, status, and boot logs, but it is visually plainer than the playback screen and the log bullets can look ragged on wide terminals.

## Approach

Upgrade the existing startup/loading renderer instead of adding a separate screen. `StartupScreenState` will carry optional download progress, and `draw_startup` will render a centered bordered panel that matches the playback screen's amber accents and blue-gray borders.

Before byte progress exists, the screen shows an indeterminate animated progress bar so the UI feels alive while metadata is resolving. Once `yt-dlp` emits progress, the same bar switches to measured percent, bytes, speed, and ETA.

## Layout

- Centered bordered panel with the ASCII logo at the top.
- Bordered status section with spinner, loading copy, and progress bar.
- Aligned log section with a fixed bullet gutter so the cheeky loading notes scan cleanly.
- Existing sync-warning banner remains above the loading panel.

## Testing

Use ratatui test backends to verify the startup screen still renders the sync-warning banner and now renders loading progress. Run `cargo test` and `cargo build`.
