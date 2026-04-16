# Paused Wave Implementation Plan

## Objective

Add a paused-only procedural wave mode to the existing scatter visualizer so the player displays a stylized Great Wave-inspired motion when audio is paused, then returns immediately to the FFT-driven scatter field when playback resumes.

## Workstreams

### 1. Paused renderer branch

- Update `draw_scatter(...)` in `src/tui.rs` to branch on `state.paused`.
- Keep the current FFT-driven rendering untouched for the active playback path.
- Route paused rendering into a dedicated helper without creating a second top-level visualizer API.

### 2. Procedural wave field

- Add helper(s) that derive dot visibility and color from:
  - row / column
  - terminal width / height
  - `frame_count`
- Model:
  - primary crest
  - trailing swell
  - foam band
  - sparse spray

### 3. Palette shift

- Add paused-wave colors:
  - deep indigo / navy body
  - slate-blue mid-water
  - warm off-white foam
- Keep the current warm-to-cool frequency gradient for active playback only.

### 4. Graceful scaling

- Ensure the paused wave reads at both small and large terminal sizes.
- Degrade to a simpler shape when there is not enough space for the full crest/spray composition.

### 5. Verification

- Build and run formatting/tests.
- Manual smoke test:
  - pause/resume in standard mode
  - pause/resume in fullscreen
  - narrow and wide terminal sizes

## File-Level Changes

- `src/tui.rs`
  - paused-wave helpers
  - paused branch inside `draw_scatter(...)`

## Sequence

1. Add paused-wave helper functions and palette helpers.
2. Branch `draw_scatter(...)` on `state.paused`.
3. Tune motion and dot density for readability.
4. Run `cargo fmt`, `cargo build`, `cargo test`.

## Risks

- The wave can become noisy or illegible in very small terminals.
- Overly literal composition may feel kitschy; the rendering should stay abstract and restrained.
- Pause/resume should not leave any stale visual state when switching back to FFT rendering.

## Acceptance Criteria

- Pausing replaces the FFT-driven scatter with a rolling wave made from the same dot medium.
- The paused palette reads as indigo / foam rather than the active frequency gradient.
- Resuming immediately returns to music-driven motion.
- Normal mode and fullscreen both use the paused wave cleanly.
