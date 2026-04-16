# Default History Startup Design

## Goal

Allow `looper` to start with no CLI parameters. In that mode, it should open a history-focused terminal UI with no active playback. Pressing `Enter` on a history row should start playback for that row's replay target using the existing playback flow.

## Approach

Use a dedicated startup mode instead of trying to fake an active track.

- Keep `looper play --url ...` working for explicit playback.
- Make the top-level CLI command optional so bare `looper` enters history-browser mode.
- Add a history-browser event loop that:
  - opens and migrates the SQLite database
  - loads history using the existing `HistoryPanelState`
  - renders a history-first screen with empty-state guidance when needed
  - exits on `q` / `Ctrl-C`
  - starts playback by returning the selected row's `replay_target` on `Enter`
- Reuse the existing playback session loop once a row is selected so replay behavior stays consistent.

## UI

The no-arg screen should not pretend something is already playing.

- Render a simple idle header and instructions above the history list.
- Keep the existing history sorting, selection, favorite toggling, and replay bindings.
- If there is no history yet, show a clear message explaining that playback history will appear after the first played track.

## Testing

- Add CLI parsing coverage for bare `looper` and explicit `play --url ...`.
- Add focused tests around the history-browser key handling if new logic needs it.
- Update README usage and key descriptions to document the new default startup behavior.
