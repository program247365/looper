# Paused Wave Design

## Goal

When playback is paused, replace the FFT-driven scatter motion with a stylized rolling wave inspired by Hokusai's Great Wave while keeping the same dot-based visual language as the active player. When playback resumes, immediately return to the audio-reactive scatter visualization.

## User Experience

- Pressing `Space` pauses audio as it does now.
- While paused, the visualizer no longer reflects audio bands.
- Instead, the scatter field becomes a procedurally animated wave scene:
  - one primary curling wave
  - one secondary trailing swell
  - pale foam near the crest
  - sparse spray drifting above the crest
- The wave continues moving while paused so the screen feels alive.
- Pressing `Space` again resumes playback and hands visual control back to FFT-driven scatter immediately.

## Visual Direction

The paused scene should read as a restrained homage rather than a literal illustration.

- Keep the same dot-based medium used by the current scatter visualizer.
- Shift the palette away from the warm-to-cool frequency gradient and into:
  - deep navy / indigo for the wave body
  - slate-blue for mid-water
  - warm off-white for foam and spray
- The primary wave should arc from the lower-left toward an upper-right curl.
- The secondary swell should sit behind the main wave to add depth and motion.
- The crest should be the brightest point in the scene.
- Background noise should stay quiet so the paused state feels composed instead of busy.

## Architecture

### Rendering approach

- Keep `draw_scatter(...)` as the single visualizer entry point.
- Branch inside it based on `state.paused`.
- Preserve the current FFT-driven renderer for the unpaused path.
- Add a paused-wave helper that synthesizes dot placement and color procedurally from:
  - terminal width/height
  - cell coordinates
  - `frame_count`

### Motion model

- Use `frame_count` as the only animation clock.
- Animate:
  - slow horizontal phase drift
  - slight breathing in crest height
  - sparse foam spray above the crest
- Avoid adding any new timing or concurrency systems.

### Data model

No new player-state structures are required if the paused renderer can derive everything from:

- `state.paused`
- `state.frame_count`
- terminal dimensions

## Error Handling

- There are no new external failure modes.
- If the terminal is too small, the paused wave should degrade gracefully into a simplified dot field rather than panic or render garbage.

## Testing

- Build-time verification should be sufficient for structure.
- Manual verification should cover:
  - pause in normal mode
  - pause in fullscreen mode
  - resume from both modes
  - small and wide terminal widths

## Scope

### In scope

- paused-only procedural wave rendering
- paused palette shift
- smooth handoff back to FFT rendering on resume

### Out of scope

- new controls
- separate illustration mode
- changes to audio playback logic
- changes to the loading UI
