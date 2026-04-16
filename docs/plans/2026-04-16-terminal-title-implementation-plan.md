# Terminal Title Animation Implementation Plan

## Objective

Add compact OSC-driven terminal title updates for loading, playing, and paused states, with minimal impact on the rest of the player.

## Workstreams

### 1. Title formatting helpers

- Add helpers for:
  - truncating long track titles
  - selecting spinner frames
  - formatting loading / playing / paused titles

### 2. OSC title writer

- Add a small helper that writes terminal title escape sequences to stdout.
- Avoid shell-specific behavior.
- Treat unsupported terminals as a no-op in practice by not depending on any acknowledgement.

### 3. Render-loop integration

- Update title from:
  - loading loop
  - playback loop
- Reuse `frame_count` for animation.
- Only write when the formatted title changes.

### 4. Exit/reset behavior

- Reset title to `looper` after normal teardown.
- Reset title on early quit and error paths as part of terminal restore flow.

## File-Level Changes

- `src/play_loop.rs`
  - title formatting/writing helpers
  - loading/playback title updates
  - reset on exit

## Sequence

1. Add formatting and OSC helpers.
2. Wire loading-title updates into the loading loop.
3. Wire playing/paused title updates into the playback loop.
4. Reset title during teardown.
5. Run formatting/build/tests.

## Risks

- Some terminals may ignore OSC title updates.
- Overly frequent writes could be noisy; only emit on actual changes.
- Long titles can make tab bars unreadable if not truncated aggressively enough.

## Acceptance Criteria

- While playing, the tab title animates and includes the current track.
- While paused, the title shows a stable paused state.
- While loading, the title animates with a loading label.
- Exiting the app restores the title to `looper`.
