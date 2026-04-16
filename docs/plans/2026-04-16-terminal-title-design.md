# Terminal Title Animation Design

## Goal

Add a lightweight animated terminal/tab title so `looper` behaves more like modern terminal apps: show a small animated glyph plus compact playback context while running, then restore the title to a neutral value on exit.

## User Experience

- While playing, the terminal tab title animates with a small rotating frame and shows the current track title.
- While paused, the title stops animating and shows a paused marker.
- While loading a remote track, the title animates and includes a `loading` label.
- On quit, error, or terminal restore, the title returns to `looper`.

## Title Formats

- playing: `◐ looper — track name`
- paused: `⏸ looper — track name`
- loading: `◓ looper — loading — track name`

Long titles should be truncated so the most useful information survives tab-bar clipping.

## Architecture

- Use OSC terminal title updates written directly to stdout.
- Drive animation from the existing `frame_count` rather than adding a new timer.
- Update only when the formatted title actually changes to avoid redundant writes.
- Keep implementation local to playback/loading orchestration rather than spreading title logic through rendering code.

## Error Handling

- If a terminal ignores OSC title updates, the app should continue normally.
- Restore the title to `looper` on normal exit, early quit, and panic-path teardown.

## Scope

### In scope

- animated playing title
- stable paused title
- animated loading title
- title reset on exit

### Out of scope

- service badges in the tab title
- playlist position in the tab title unless space is abundant
- shell-specific integrations
