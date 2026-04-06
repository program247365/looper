# WORKLOG

## 2026-04-06: Add ratatui TUI with rodio audio backend

### What changed
- Replaced `play` crate with `rodio` (0.17) for proper audio control (pause/resume, duration probing, `repeat_infinite()` looping)
- Added `ratatui` (0.26) + `crossterm` (0.27) for the terminal UI
- New `src/audio.rs`: `AudioPlayer` struct wrapping `OutputStream` + `Sink`; `_stream` field keeps the audio device alive for the process lifetime
- New `src/tui.rs`: `AppState`, `setup_terminal`/`restore_terminal`, `draw()` with 4-section layout (header, progress bar, animated visualizer, footer)
- Rewrote `src/play_loop.rs` as a 100ms-tick crossterm event loop; tracks loop count via `Instant` elapsed vs. duration (not `Sink::get_pos()` which doesn't reset on `repeat_infinite`)
- Added panic hook to restore terminal before printing panic output
- Keys: `Space` pause/resume, `q` or `Ctrl+C` quit
- Visualizer is simulated (two sine waves per bar, no FFT) — looks good, no audio sample access needed
- Updated `CLAUDE.md` to reflect new architecture and `make` commands
- Added `Makefile` reference (added in prior commit by user)

### What we decided
- Keep `structopt` rather than migrating to `clap` v4 — not in scope
- Simulated visualizer over real FFT — adding FFT would require intercepting the rodio decode pipeline; not worth the complexity for a loop tool
- Duration tracked via `Instant` on main thread, not `Sink::get_pos()`, because `repeat_infinite()` doesn't reset the sink position counter between loops
- `is_paused()` removed from `AudioPlayer` — pause state is tracked in `AppState`, no need to query rodio

### What to revisit
- `total_duration()` returns `None` for some VBR MP3s — progress bar now shows elapsed correctly; total still shows `--:--`
- Loop count stays at 1 when duration is unknown — acceptable for now
- Could add volume control (`+`/`-` keys → `sink.set_volume()`) as a follow-up

## 2026-04-06: Real FFT visualizer + progress time fix

### What changed
- Added `spectrum-analyzer` (1.7) for FFT-based audio analysis
- New `SampleTap<S>` wrapper in `audio.rs`: intercepts samples on rodio's audio thread via `Iterator::next()`, writes to a shared `VecDeque<f32>` ring buffer (8192 cap). Uses `try_lock()` so audio thread never blocks.
- `AudioPlayer` now exposes `sample_buf`, `sample_rate`, `channels`
- `update_visualizer()` in `play_loop.rs`: reads latest 2048 mono samples (down-mixing stereo), applies Hann window, runs FFT, maps bins to 32 log-spaced bands (20 Hz–20 kHz), applies asymmetric smoothing (attack 0.6, decay 0.25)
- Visualizer rewritten as multi-row multi-color bar chart: green (bass) → yellow (mids) → red (treble), 7 inner rows, fills from bottom
- Progress bar now shows elapsed time even when `total_duration()` returns `None` (most VBR MP3s): `0:12 / --:--` instead of `--:-- / --:--`
- Tick rate increased from 100ms to 50ms for more responsive visualizer

### What we decided
- Use `try_lock()` not `lock()` in `SampleTap::next()` — audio thread must never block; occasional missed samples don't matter
- `spectrum-analyzer` chosen over raw `rustfft` — bundles Hann window support and handles windowing/bin extraction cleanly
- Scale multiplier of 8.0 on raw FFT magnitude — tuned empirically; may need adjustment for very quiet/loud tracks
- 32 bands, 2048 FFT window — good resolution without latency

### What to revisit
- Scale factor (8.0) might need tuning per-track; could auto-normalize based on rolling max
- Could add volume control (`+`/`-` keys → `sink.set_volume()`)
- Color gradient could be more granular (lerp across RGB rather than 3 discrete zones)
