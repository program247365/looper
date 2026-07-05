# Spotify Search Overlay

**Date:** 2026-07-03
**Status:** Approved (amended 2026-07-05)

> **Amendment (2026-07-05):** the "mint a Web API token from the librespot
> session" plan failed in live testing — Spotify's Mercury keymaster endpoint
> is retired (403), and login5 tokens get 429 on every `api.spotify.com`
> endpoint (with or without a `client-token` attestation header); Mercury
> `searchview` is 404. Search instead uses the client-credentials flow with a
> user-provided free Spotify API app (`SPOTIFY_CLIENT_ID` /
> `SPOTIFY_CLIENT_SECRET` env vars), with the token cached in-process for its
> ~1h lifetime. Missing credentials render setup instructions in the overlay.
> Search is now independent of the Premium login (needed only for playback).

## Summary

Add in-TUI Spotify search. `/` opens a search overlay from the playback screen
or the history browser. The user types a query, presses Enter to search, then
navigates results with vim keys and presses Enter to play the selected song,
album, or playlist. Selection reuses the existing replay-target rail: the play
loop returns the selected `spotify:` URI and the session loop re-resolves and
plays it, so playlist semantics, looping, history recording, and album art all
come from existing code.

Interaction model: submit-to-search (one blocking Web API call per Enter), not
live search-as-you-type.

## Search backend — `src/spotify/search.rs` (new)

`pub fn search(query: &str) -> Result<SearchResults>`

- Uses the shared librespot session (`spotify::session()`), which already
  handles reconnect-from-cached-credentials.
- Mints a Web API bearer token via `session.token_provider().get_token(...)`.
  No developer app or second OAuth flow — the token comes from the existing
  Premium login.
- One request:
  `GET https://api.spotify.com/v1/search?q=<query>&type=track,album,playlist&limit=8`
  with `Authorization: Bearer <token>`, sent with the reqwest client already in
  the tree (`stream_download::http::reqwest`), executed via
  `ctx.runtime.block_on` — the same pattern `spotify::resolve` uses.
- Parses with `serde_json` into:

```rust
pub struct SearchResults {
    pub tracks: Vec<SearchItem>,
    pub albums: Vec<SearchItem>,
    pub playlists: Vec<SearchItem>,
}

pub struct SearchItem {
    pub title: String,
    pub byline: String,  // artist(s), or playlist owner
    pub detail: String,  // "3:42" for tracks, "24 tracks" for collections
    pub uri: String,     // "spotify:track:<id>" / "spotify:album:<id>" / "spotify:playlist:<id>"
}
```

- Filters `null` entries defensively (Spotify's search API returns them in
  playlist results since the 2024 editorial-content changes).
- Display truncation: 8 tracks, 5 albums, 5 playlists.
- Not logged in → error result carrying the existing
  "run `looper spotify login` first" message; shown in the overlay, no crash.

## TUI state and rendering — `src/tui.rs`

New state alongside `HistoryPanelState`:

```rust
pub struct SearchPanelState {
    pub input: String,
    pub focus: SearchFocus,            // Query (typing) | Results (navigating)
    pub results: Option<SearchResults>,
    pub selected: usize,               // index into flattened selectable rows
    pub status: SearchStatus,          // Idle | Searching | Error(String)
}
```

`draw_search_panel` renders an overlay in the history-panel visual style:

- query input line at the top
- one flat list below with `SONGS` / `ALBUMS` / `PLAYLISTS` section headers
- headers are not selectable; `j`/`k` skip over them
- each row: title, byline, detail
- a "searching…" frame with spinner is drawn before the blocking search call
- error line (from `SearchStatus::Error`) preserves the query for retry

## Key handling — `src/play_loop.rs`

New `KeyCommand` variants, following the existing `History*` style:
`SearchOpen`, `SearchChar(char)`, `SearchBackspace`, `SearchSubmit`,
`SearchNext`, `SearchPrev`, `SearchTop`, `SearchBottom`, `SearchClose`,
`SearchPlay`.

Bindings:

- Playback scene and history browser: `/` → `SearchOpen`.
- Query focus: printable chars append; Backspace deletes; Enter submits the
  search; Esc closes the overlay.
- Results focus: `j`/`k` move; `gg`/`G` jump to top/bottom; `/` returns to
  query focus to edit and re-search; Enter plays the selection; Esc closes.

`handle_key_event` checks the search panel before the history panel, so the
overlay captures all keys while open (e.g. `j` never leaks to history
navigation, `q` never quits mid-typing).

The overlay's key handling and state transitions live in one shared helper
used by both the playback loop and `browse_history_session`, not duplicated.

## Playback handoff

No new resolution code. `spotify::resolve` already dispatches on
track/playlist/album URIs.

- Playback loop: `SearchPlay` makes `run_loop` return
  `Some(selected_uri)`; the session loop in `play_file_session` re-resolves
  and plays it — identical to history replay. Track URIs loop forever;
  album/playlist URIs get playlist semantics.
- History browser: `SearchPlay` calls `play_file_session` with the selected
  URI, exactly like `HistoryReplay` does today.
- History recording, Now Playing metadata, and album art are downstream of
  `resolve` and untouched.

## Error handling

- Search failure (network, token, HTTP 4xx/5xx) → `SearchStatus::Error` shown
  in the overlay; the query is preserved; playback is never torn down.
- Not logged in → same overlay error path with the login hint.
- Empty results → overlay shows "no results" in place of the list.
- Blocking call cost: ~300ms; audio is unaffected (rodio owns its own
  thread); only the visualizer stalls for the duration.

## Testing

- Serde parsing test with a JSON fixture of a real `/v1/search` response,
  including `null` playlist entries (verifies filtering).
- Key-routing tests in the existing `play_loop.rs` KeyEvent test style:
  `/` opens, typing edits the query, Enter submits, `j`/`k` navigation skips
  headers, Enter selects, Esc closes.
- Manual smoke test: search a query, play one track, one album, one playlist;
  confirm history records each and Esc/close never corrupts the terminal.

## Out of scope

- Live search-as-you-type (debounced background search) — possible later
  upgrade; nothing in this design blocks it.
- Category tabs (`h`/`l` between Songs/Albums/Playlists) — can be layered on
  without rework.
- Searching non-Spotify services.
- A search-first launch mode (`looper search` / no-URL launch).

## Estimated shape

One new file (~150 lines), moderate additions to `src/tui.rs` and
`src/play_loop.rs`, no new dependencies.
