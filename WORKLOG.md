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

## 2026-04-06: Scatter visualizer, fullscreen mode, Homebrew distribution

### What changed
- Rewrote visualizer as scatter/particle dot style: deterministic per-cell hash (`cell_noise`) for stable non-flickering dots; quadratic density falloff toward amplitude ceiling
- Color gradient: pink (bass) → amber → yellow → lime → cyan (treble) via `Color::Rgb(r,g,b)`
- Added fullscreen toggle (`f` key): full-window scatter with micro-status bar at bottom
- Replaced Gauge widget progress bar with custom Paragraph: `━━━●──── 0:42/3:12` style
- Added symphonia fallback for VBR MP3 duration (`probe_duration_symphonia` using Xing/VBRI headers); total time now shows correctly for most MP3s
- Added `libc` dep and TTY reattachment (`/dev/tty` dup2) so crossterm works when stdin is a pipe (xargs invocation)
- Fixed `mzk` dotfiles function: now captures `sk` output in `$()` then invokes looper directly instead of piping via xargs
- Published v0.1.0 Homebrew tap: `program247365/homebrew-tap` with `Formula/looper.rb`
- Added `brew tap program247365/tap` + `brew 'program247365/tap/looper'` to `~/.dotfiles/Brewfile`
- Updated `Makefile` with `release`, `release-patch`, `release-minor`, `bump-formula` targets
- Updated `README.md` with Homebrew install, keys table, dev commands, release workflow

### What we decided
- Deterministic cell hash over random noise — prevents flickering on each redraw tick
- Scatter over bar chart — closer to the reference screenshot the user provided
- symphonia was already a transitive dep via rodio; adding it as explicit dep costs nothing
- `/dev/tty` reattachment is a defensive measure; the real fix is in `mzk` but belt-and-suspenders is fine here

### What to revisit
- Volume control (`+`/`-` keys → `sink.set_volume()`)
- Auto-normalize FFT scale based on rolling max amplitude per track
- RGB lerp gradient instead of discrete color stops

## 2026-04-06: Visualizer — animated twinkling, per-band AGC, smoother gradient

### What changed
- `frame_count: u64` added to `AppState`; incremented unconditionally each tick (including when paused)
- `cell_noise` now takes `t: usize` (= `frame_count / 4`); dot pattern shifts every ~120ms creating gentle shimmer on all bands, including sustained bass
- `band_peak: Vec<f32>` added to `AppState`; per-band rolling max with 0.998 decay (noise floor 0.02)
- FFT bin aggregation changed from mean → max; improves sensitivity for bass bands with 1-2 sparse bins
- Scale factor `* 8.0` removed; replaced by per-band peak normalization (`raw_mag / band_peak[i]`)
- Decay smoothing: `0.25/0.75` → `0.35/0.65` (snappier falloff)
- Tick rate: 50ms → 30ms (~33 Hz)
- `scatter_color` rewritten with RGB lerp through 5 stops (pink → amber → yellow → lime → cyan); no more hard zone boundaries

### What we decided
- Twinkling is the right solution for sustained signals — the FFT *correctly* shows constant energy for a sustained bass pad; animation adds life without lying about the signal
- Per-band AGC ensures the visualizer works well across all genres; a jazz bass guitar and an 808 kick both use the full visual range for their respective band
- `frame_count / 4` divisor at 30ms tick = 120ms shimmer rate; empirically feels organic vs. noisy

### What to revisit
- Volume control (`+`/`-` keys → `sink.set_volume()`)
- Amplitude-coupled twinkle speed (louder bands twinkle faster)
- Beat-flash: brief brightness boost on kick drum detection
