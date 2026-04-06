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
- `total_duration()` returns `None` for some VBR MP3s — progress bar shows `--:--` in that case; could add a symphonia-based duration probe as fallback
- Loop count stays at 1 when duration is unknown — acceptable for now
- Could add volume control (`+`/`-` keys → `sink.set_volume()`) as a follow-up
