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

## 2026-07-05: In-TUI Spotify search (`/`) — searched, navigated, and looped from the terminal

### What changed
- `/` opens a Spotify catalog search overlay from the playback screen and the history browser (`src/spotify/search.rs`, overlay rendering in `src/tui.rs`, key routing in `src/play_loop.rs`).
- Submit-to-search model: type query, Enter runs one blocking Web API call (~300ms, "searching…" frame first); results grouped SONGS/ALBUMS/PLAYLISTS.
- Vim navigation: `j`/`k` (skips section headers), `gg`/`G`, `/` re-edits the query, Enter plays the selection, Esc closes. The overlay captures all keys while open (only Ctrl-C quits).
- Selection rides the existing replay rail (`LoopAction::ReplayTarget` / `play_file_session`), so resolve, track-vs-playlist looping, history recording, and album art needed zero new code.
- Search auth: user-supplied Spotify API app via `SPOTIFY_CLIENT_ID`/`SPOTIFY_CLIENT_SECRET` (client-credentials flow, token cached in-process ~1h). Missing vars → overlay shows setup steps. Docs in README + docs/spotify.md "Search (optional)".

### What we decided and why
- Spec'd a zero-setup path (mint Web API tokens from the librespot session) — it died in live testing: Mercury keymaster 403 (retired), login5 tokens get 429 on every api.spotify.com endpoint (with or without client-token attestation), Mercury searchview 404. Spotify has effectively blocked its public Web API for librespot's shared client id — same wall spotify-player hit. Amended the spec in place.
- Client-credentials over PKCE: no browser flow, no redirect-URI registration, search needs no user context. Search is now independent of the Premium login (playback still needs it).
- Chose submit-to-search over live search-as-you-type (worker thread + stale-result handling not worth it for v1) and sectioned list over tabs (tabs can layer on later).
- librespot 0.8 gotcha: `TokenProvider::get_token` takes a comma-separated `&str`, not a slice (docs.rs rendering misleads).

### What to revisit
- Live search-as-you-type and/or category tabs if the sectioned list feels cramped.
- Search-first launch mode (`looper search` or `/` from a bare `looper` before the history browser had anything to play).
- reqwest `429`/Retry-After handling in search (currently surfaces as an error in the overlay; fine for personal API apps with generous quotas).
- Machine note: this Mac had no Rust toolchain; installed rustup (stable 1.96.1). `cargo` lives in `~/.cargo/bin` — new shells should pick it up via the rustup env hooks.

## 2026-07-05: History panel default sort buried just-played tracks — switched to Last Played

### What changed
- `HistoryPanelState::fresh()` constructor (`src/tui.rs`) — fresh history panels now default to sorting by Last Played descending instead of Time Played descending. Both construction sites (`browse_history_session`, `toggle_history_panel` in `src/play_loop.rs`) use it.
- Regression test `fresh_panel_surfaces_just_played_track_first` (`src/tui.rs`) — records an old track with 70k accumulated seconds and a brand-new play, asserts the new play sorts first under the fresh-panel default. Mutation-verified (fails under the old TimePlayed default).

### What we decided and why
- Bug report was "Spotify search play not tracked in history" — it *was* tracked (verified in `looper.sqlite3`: record lands at playback start). The default Time Played sort buried it: `total_play_seconds` only accumulates when a track ends (`persist_played_time`), so a first-listen track sorts to the very bottom of a descending list and looks missing.
- Fixed the default sort rather than making play-seconds accumulate live: a "previously played" list should show recency first, and even live accumulation would rank a new track below multi-hour veterans. Sort remains cyclable with the existing keys.

### What to revisit
- Consider persisting `total_play_seconds` incrementally (e.g. every N seconds) so the Time Played sort is honest mid-session and a crash doesn't lose the session's playtime.
- Playlists/albums are recorded per-track only; the collection itself never appears in history. If "replay that whole playlist" from history matters, that needs a collection-level record.

## 2026-07-05: Collection history rows — playlists/albums are replayable from history

### What changed
- Migration `2026-07-05-000004_add_kind`: `played_tracks.kind TEXT NOT NULL DEFAULT 'track'` (`'track' | 'collection'`), surfaced as `RecordKind` through `TrackRecord`/`HistoryRow`.
- A playlist/album launch records a collection row (`collection_record` in `storage.rs`, called from `play_tracks`): keyed by the requested URL so `enter` in the history browser re-resolves and replays the whole thing. `play_count` = launches, not loop passes.
- Played seconds accrue to both the track row and its collection row (`collection_key` threaded through `play_single_track`), so the Time Played sort ranks collections honestly.
- yt-dlp entries now parse `playlist_title`/`playlist` into `MetadataEntry.collection` and stamp it on `TrackInfo` — YouTube/SoundCloud playlists get real collection names in history, the playback header, and Now Playing. Single-JSON fallback stamps the top-level playlist title.
- Collection rows render with a `≡ ` prefix and a warm tint in the history table.
- Tests: kind round-trip, `collection_record` title/URL-fallback, `parse_entry` playlist-title parsing, glyph render test. Migration verified against a copy of the real 19-row DB (all rows backfill as `track`).

### What we decided and why (grill session)
- Record collection + per-track rows (not collection-only): keeps favorites/replay on individual tracks, no regression.
- Same table + `kind` column over a separate `played_collections` table: shared favorite/delete/sort/replica code, one small migration.
- Record at launch, consistent with per-track behavior — the "record at end" variant was rejected as the same class of bug as the sort issue fixed earlier today.
- Delete on a collection row prunes only that row (no membership stored, no cascade).

### What to revisit
- Manual smoke test pending: play a Spotify album and a YouTube playlist, check the ≡ rows appear and replay.
- 1-track playlists take the single-track path and record no collection row.
- Old-version binaries opening a migrated DB are fine (diesel selects by name, inserts get the SQL default); replica last-write-wins across versions unchanged.

## 2026-07-05: Search missed most of an artist's albums — added ARTISTS section + full-discography browse
- Investigated "The Toxic Avenger" showing 3 of 14 albums: `/v1/search` is relevance-ranked
  text search (681 album matches for that query), the request asked for 8, and the overlay
  displayed only 5 (`ALBUM_LIMIT`). Two fetched albums were silently dropped by the display cap.
- Live-probed the Web API: dev-mode apps now reject `limit` > 10 (400 "Invalid limit",
  2025 restriction) — documented in CLAUDE.md; search + discography paging pinned to 10.
- Changes: `ALBUM_LIMIT` 5→8; request `limit` 8→10; `type=artist` added to search; new
  ARTISTS section (3 max) between SONGS and ALBUMS. Enter on an artist row fetches
  `/v1/artists/{id}/albums` (paged by 10, capped at 50) and swaps the overlay to a grouped
  discography (ALBUMS / SINGLES & EPS / COMPILATIONS) via `panel.pending_artist` and the
  existing "searching…" deferred-execute rail. `/` + Enter re-runs the search to go back.
- Decided: text search can never guarantee an artist's complete catalog — the artist-albums
  endpoint is the only authoritative source, so browse-by-artist is a distinct flow, not a
  bigger search limit. Discography cap bounds TUI-thread blocking (~5 sequential calls max).
- Verified: TDD (parser/flatten tests RED→GREEN), plus live smoke tests
  (`search_smoke`, `discography_smoke` — 14 albums returned for the artist).
- Revisit: mid-list "searching…" hint says the same thing for search and discography;
  singles tail truncates silently past 50 entries; artist rows have no byline/detail
  (dev-mode search omits follower counts).

## 2026-07-07: Favorites sort mode in the history browser h/l cycle
- Added `HistorySortField::Favorites` (label "Favorites") between Last Played and
  Platform in the h/l cycle, with comparator `is_favorite` → `last_played_at` → `title`.
- Decided: a plain sort mode, not a pin-favorites overlay — one axis of state, and
  `r` reverse stays consistent across all modes. Within the starred group, most
  recently played sorts first (user's pick).
- Note: `descending` is a whole-list reverse after an ascending sort, so the
  comparator treats favorite as *greater*; the default descending panel is what
  floats stars to the top.
- Verified: two new storage tests (starred-above-unstarred via `list_history`,
  in-group recency via `compare_history_rows`); full `cargo test` green.
