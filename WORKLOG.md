# WORKLOG

## 2026-04-16: Online playback, loading UI, service badges

### What changed
- Added remote URL support through `src/plugin/` with service matchers for YouTube, SoundCloud, and HypeM plus a generic `yt-dlp` fallback
- Added OS cache directory support via `directories::ProjectDirs`
- Local files still play directly, but remote tracks now resolve into `TrackInfo` values that can point at cached files, HTTP streams, or process-backed streams
- Added playlist orchestration in `play_loop.rs`: single tracks loop forever; playlists play each track once and then loop the entire list
- Added bounded background prefetch for playlist tracks
- Added a full-screen loading scene for uncached remote startup with progress, bytes, speed, and ETA
- Added compact cache status support in the playback header
- Added small source badges in the TUI: `YT`, `SC`, `HM`
- Added clearer `yt-dlp` error reporting, especially around YouTube `403` failures and invalid HypeM URLs

### What we decided
- Keep YouTube on a download-first cached path for now because direct/process streaming was less reliable than cached playback with current `yt-dlp` behavior
- Keep SoundCloud and HypeM on the newer hybrid path: prefer stream-first where workable, fall back to download-first
- Put remote startup progress inside the TUI instead of relying on stderr logging
- Use text badges instead of terminal image/SVG rendering for source icons; simpler and much more reliable

### What to revisit
- Cookie/authenticated `yt-dlp` support for YouTube when anonymous access fails
- More robust in-player cache progress after the loading screen handoff
- Optional fallback from text badges to Nerd Font glyphs if portability concerns are acceptable
- More live-service smoke coverage across public playlist URLs

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

## 2026-04-30: macOS media keys + system Now Playing widget

### What changed
- Added `souvlaki = "0.8"` (with `use_zbus`, no `libdbus-1-dev` needed on Linux); macOS-only deps `cocoa` + `objc` to drive `NSApplication.run()`
- New `src/media_controls.rs` cross-platform façade: `MediaSession::start() -> (MediaSession, Receiver<KeyCommand>)`, `MediaSessionHandle::{set_metadata, set_playback}` (cheap-cloneable `Arc<Mutex<MediaControls>>`)
- New `src/macos_runloop.rs`: spawns a `looper-tui` worker thread, runs `NSApp.run()` on the main thread (activation policy `Accessory`, no Dock icon). Worker calls `std::process::exit` on completion.
- Refactored `play_loop.rs` to thread a `PlaybackContext { cmd_rx: &Receiver<KeyCommand>, media: Option<MediaSessionHandle> }` through `play_file → play_file_session → play_tracks → loop_playlist/play_single_track → run_loop`
- Extracted dispatch logic from `run_loop` into a `dispatch_command` helper so keyboard events and external (media-key) events flow through one match
- Added `KeyCommand::{NextTrack, PreviousTrack}`, `LoopAction::PreviousTrack`, `AudioPlayer::skip()` (calls `Sink::stop()`)
- Playlist loop now uses `while idx < total_tracks` with `idx.saturating_sub(1)` for Previous (restarts track 0 if pressed there) instead of a `for` range
- Bound `n` (Next) and `b` (Previous) keyboard shortcuts in playback mode so the playlist control surface is testable without media keys
- Updated `--help` to document new keys + macOS media-key behavior
- Phase 2: `set_metadata(&track)` on each track start, `set_playback(paused, elapsed)` on TogglePause — populates Control Center / lock screen / AirPods Now Playing widget

### What we decided
- Use one crate (`souvlaki`) for all three OSes rather than per-platform glue. The dep is not `#[cfg]`-gated; only the *setup code* is.
- macOS thread-flip via `NSApp.run()` + worker `process::exit`. Considered `CFRunLoopStop`+`CFRunLoopWakeUp` and `[NSApp stop:]`+`postEvent` — both add boilerplate; `process::exit` after the worker has cleanly run terminal-restore is observably equivalent and far simpler.
- Skip a custom NSStatusItem (menu-bar text) — the souvlaki integration already populates the system Now Playing widget which is the iTunes-equivalent on modern macOS. Custom NSStatusItem would duplicate that surface.
- Defer Windows: would need a hidden message-only HWND + per-tick `pump_event_queue` (souvlaki ships an example). Additive change; ship later if anyone asks.
- `use_zbus` over default `use_dbus` so Linux builds don't pull in `libdbus-1-dev` system package — better for distro packaging.

### What to revisit
- Manual smoke test on real Mac hardware: F8 (Play/Pause), F7/F9 (Prev/Next), Control Center widget, AirPods double-tap, lock screen, terminal resize during playback, q + Ctrl-C clean exit.
- Souvlaki [issue #77](https://github.com/Sinono3/souvlaki/issues/77) (debug-build panic on macOS, open). Run release-build smoke if debug crashes.
- Track artwork in Now Playing — yt-dlp metadata has thumbnail URLs we could pass to `MediaMetadata.cover_url`.
- Live progress updates in the widget (currently set on track-change and pause/resume only). Could push `set_playback` once per second from the TUI tick.
- Graceful NSApp shutdown if we ever care about Drop-running for `MediaControls` (currently sidestepped via `process::exit`).
