# Arrow-Key Seek Design

## Goal

During playback, pressing the right arrow seeks five seconds forward in the
current track. Pressing the left arrow seeks five seconds backward.

## Behavior

- Arrow keys work on the normal playback screen.
- The played-songs panel and default history browser keep their existing key
  behavior.
- Playlist navigation remains on `n` and `b`; arrow keys seek within the
  current playlist track.
- Seeking clamps at the start of the track and, when duration is known, the end
  of the track.
- Seeking applies to local files and cached downloads. Live or stream-first
  sources without a local seekable file ignore the arrow keys.

## Implementation

Add seek-forward and seek-back commands to playback input handling, implement a
small seek primitive in the audio player, and keep `AppState` elapsed-time
tracking in sync with the new playback position.

## Verification

Add focused unit coverage for arrow-key command mapping and seek clamping.
Run the Rust test suite after implementation.
