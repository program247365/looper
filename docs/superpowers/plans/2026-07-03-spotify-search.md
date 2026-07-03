# Spotify Search Overlay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/` opens a Spotify search overlay (playback screen and history browser); type a query, Enter searches, vim keys navigate, Enter plays the selected track/album/playlist through the existing replay rail.

**Architecture:** A new `src/spotify/search.rs` calls Spotify's Web API `/v1/search` with a bearer token minted from the existing librespot session. TUI state (`SearchPanelState`) and rendering live in `src/tui.rs` following the history-panel pattern. Key routing and command dispatch extend `KeyCommand` in `src/play_loop.rs`; playing a selection returns `LoopAction::ReplayTarget(uri)` so all resolution/looping/history code is reused untouched.

**Tech Stack:** Rust, ratatui/crossterm, librespot 0.8 (`Session::token_provider()`), reqwest (via `stream_download::http::reqwest`), serde_json.

**Spec:** `docs/superpowers/specs/2026-07-03-spotify-search-design.md`

## Global Constraints

- No new Cargo dependencies. reqwest comes from `stream_download::http::reqwest`; JSON from `serde_json`.
- Do NOT run `cargo update` (vergen is pinned to 9.0.6; see Cargo.toml note).
- All blocking async work goes through the shared Spotify runtime (`ctx.runtime.block_on`), same as `spotify::resolve`.
- Display truncation: 8 tracks, 5 albums, 5 playlists.
- Search failures must never tear down playback or the terminal — errors render in the overlay.
- Follow existing code style: sparse comments only for non-obvious constraints, match the history-panel idioms.
- Verify each task with `cargo build` and `cargo test` (non-interactive; audio tests stay ignored).
- Commit messages: conventional-commit style like existing history (`feat(...)`, `fix(...)`).

---

### Task 1: Search backend — types, parsing, Web API call

**Files:**
- Create: `src/spotify/search.rs`
- Modify: `src/spotify/mod.rs` (add `mod search;` + re-export; no other changes)

**Interfaces:**
- Consumes: `super::ctx()`, `super::session()` (crate-private helpers in `src/spotify/mod.rs`).
- Produces (used by Tasks 2–5):
  - `crate::spotify::search(query: &str) -> color_eyre::eyre::Result<SearchResults>`
  - `pub struct SearchResults { pub tracks: Vec<SearchItem>, pub albums: Vec<SearchItem>, pub playlists: Vec<SearchItem> }`
  - `pub struct SearchItem { pub title: String, pub byline: String, pub detail: String, pub uri: String }` (derives `Clone, Debug, PartialEq`)

- [ ] **Step 1: Write the failing parser test**

Create `src/spotify/search.rs` containing only the test module for now:

```rust
//! Spotify catalog search via the public Web API.
//!
//! Playback and metadata go through librespot's own protocols, but librespot
//! exposes no search. The Web API's `/v1/search` does, and the librespot
//! session can mint a bearer token for it — so search needs no second login
//! and no developer app.

#[cfg(test)]
mod tests {
    use super::*;

    // Trimmed real-shape /v1/search response. The null playlist entry is
    // deliberate: Spotify returns nulls in playlist items since the 2024
    // editorial-content changes.
    const FIXTURE: &str = r#"{
        "tracks": { "items": [
            { "name": "Windowlicker", "duration_ms": 366000,
              "artists": [{ "name": "Aphex Twin" }],
              "uri": "spotify:track:5MMWpTWKyTUottUuQxRXVx" }
        ] },
        "albums": { "items": [
            { "name": "Syro", "total_tracks": 12,
              "artists": [{ "name": "Aphex Twin" }],
              "uri": "spotify:album:1WuUwNAeBHEIxdXK2mmzvL" }
        ] },
        "playlists": { "items": [
            null,
            { "name": "Aphex Twin Essentials", "uri": "spotify:playlist:37i9dQZF1DZ06evO2iBPiw",
              "owner": { "display_name": "Spotify" },
              "tracks": { "total": 50 } }
        ] }
    }"#;

    #[test]
    fn parses_search_response() {
        let results = parse_search_response(FIXTURE).unwrap();
        assert_eq!(
            results.tracks,
            vec![SearchItem {
                title: "Windowlicker".into(),
                byline: "Aphex Twin".into(),
                detail: "6:06".into(),
                uri: "spotify:track:5MMWpTWKyTUottUuQxRXVx".into(),
            }]
        );
        assert_eq!(results.albums[0].detail, "12 tracks");
        // null playlist entry filtered, real one kept
        assert_eq!(results.playlists.len(), 1);
        assert_eq!(results.playlists[0].byline, "Spotify");
        assert_eq!(results.playlists[0].detail, "50 tracks");
    }

    #[test]
    fn truncates_to_display_limits() {
        let many_tracks: Vec<String> = (0..20)
            .map(|i| format!(
                r#"{{ "name": "T{i}", "duration_ms": 1000, "artists": [], "uri": "spotify:track:{i}" }}"#
            ))
            .collect();
        let json = format!(
            r#"{{ "tracks": {{ "items": [{}] }} }}"#,
            many_tracks.join(",")
        );
        let results = parse_search_response(&json).unwrap();
        assert_eq!(results.tracks.len(), 8);
        assert!(results.albums.is_empty());
        assert_eq!(results.tracks[0].byline, ""); // empty artists → empty byline
    }
}
```

Register the module in `src/spotify/mod.rs` right after `mod sink;`:

```rust
mod search;

pub use search::{search, SearchItem, SearchResults};
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --lib spotify::search -- --nocapture` (or `cargo test parses_search_response`)
Expected: COMPILE ERROR — `parse_search_response` / `SearchItem` not found. That is the failure we want.

- [ ] **Step 3: Implement types and parsing**

Add above the test module in `src/spotify/search.rs`:

```rust
use color_eyre::eyre::{eyre, Result, WrapErr};
use serde::Deserialize;
use stream_download::http::reqwest::Client as HttpClient;

/// Display limits per section (spec: 8 tracks, 5 albums, 5 playlists).
const TRACK_LIMIT: usize = 8;
const ALBUM_LIMIT: usize = 5;
const PLAYLIST_LIMIT: usize = 5;

#[derive(Debug, Default)]
pub struct SearchResults {
    pub tracks: Vec<SearchItem>,
    pub albums: Vec<SearchItem>,
    pub playlists: Vec<SearchItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SearchItem {
    pub title: String,
    /// Artist(s) for tracks/albums, owner for playlists.
    pub byline: String,
    /// "3:42" for tracks, "12 tracks" for albums/playlists.
    pub detail: String,
    /// Canonical `spotify:` URI — a valid `spotify::resolve` target.
    pub uri: String,
}

#[derive(Deserialize)]
struct ApiResponse {
    #[serde(default)]
    tracks: ApiPage<ApiTrack>,
    #[serde(default)]
    albums: ApiPage<ApiAlbum>,
    #[serde(default)]
    playlists: ApiPage<ApiPlaylist>,
}

/// A paging object. Items are `Option` because the playlist search returns
/// literal nulls for entries Spotify no longer exposes.
#[derive(Deserialize)]
#[serde(default)]
struct ApiPage<T> {
    items: Vec<Option<T>>,
}

impl<T> Default for ApiPage<T> {
    fn default() -> Self {
        ApiPage { items: Vec::new() }
    }
}

#[derive(Deserialize)]
struct ApiArtist {
    name: String,
}

#[derive(Deserialize)]
struct ApiTrack {
    name: String,
    duration_ms: u64,
    #[serde(default)]
    artists: Vec<ApiArtist>,
    uri: String,
}

#[derive(Deserialize)]
struct ApiAlbum {
    name: String,
    total_tracks: u64,
    #[serde(default)]
    artists: Vec<ApiArtist>,
    uri: String,
}

#[derive(Deserialize)]
struct ApiOwner {
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct ApiPlaylistTracks {
    total: u64,
}

#[derive(Deserialize)]
struct ApiPlaylist {
    name: String,
    uri: String,
    owner: Option<ApiOwner>,
    tracks: Option<ApiPlaylistTracks>,
}

fn join_artists(artists: &[ApiArtist]) -> String {
    artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_track_duration(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{}:{:02}", secs / 60, secs % 60)
}

fn parse_search_response(body: &str) -> Result<SearchResults> {
    let response: ApiResponse =
        serde_json::from_str(body).wrap_err("unexpected Spotify search response")?;

    let tracks = response
        .tracks
        .items
        .into_iter()
        .flatten()
        .take(TRACK_LIMIT)
        .map(|t| SearchItem {
            byline: join_artists(&t.artists),
            detail: format_track_duration(t.duration_ms),
            title: t.name,
            uri: t.uri,
        })
        .collect();

    let albums = response
        .albums
        .items
        .into_iter()
        .flatten()
        .take(ALBUM_LIMIT)
        .map(|a| SearchItem {
            byline: join_artists(&a.artists),
            detail: format!("{} tracks", a.total_tracks),
            title: a.name,
            uri: a.uri,
        })
        .collect();

    let playlists = response
        .playlists
        .items
        .into_iter()
        .flatten()
        .take(PLAYLIST_LIMIT)
        .map(|p| SearchItem {
            byline: p
                .owner
                .and_then(|o| o.display_name)
                .unwrap_or_default(),
            detail: p
                .tracks
                .map(|t| format!("{} tracks", t.total))
                .unwrap_or_default(),
            title: p.name,
            uri: p.uri,
        })
        .collect();

    Ok(SearchResults {
        tracks,
        albums,
        playlists,
    })
}
```

Note: `serde` is not currently a direct dependency (only `serde_json`). Check `Cargo.toml`; if `serde` with `derive` is absent, derive-free parsing via `serde_json::Value` is NOT the fallback — instead add nothing new: `serde` is already in the tree as a transitive dependency of `serde_json`, but the `derive` feature may not be enabled. If `use serde::Deserialize` fails to compile, change the `[dependencies]` line for serde_json to add `serde = { version = "1", features = ["derive"] }`. This is the one permitted exception to "no new dependencies" — it is already compiled into the tree via other crates, so build cost is zero.

- [ ] **Step 4: Run the parser tests**

Run: `cargo test --lib spotify::search`
Expected: both tests PASS.

- [ ] **Step 5: Implement the network call**

Add to `src/spotify/search.rs` (below the types, above the tests):

```rust
/// Search Spotify's catalog. Blocking: runs one Web API request on the shared
/// Spotify runtime (~300ms). Requires a prior `looper spotify login`.
pub fn search(query: &str) -> Result<SearchResults> {
    let ctx = super::ctx()?;
    let session = super::session()?;
    let body = ctx.runtime.block_on(async move {
        let token = session
            .token_provider()
            .get_token(&["user-read-private", "playlist-read-private"])
            .await
            .map_err(|e| eyre!("failed to get Spotify API token: {e}"))?;
        let response = HttpClient::new()
            .get("https://api.spotify.com/v1/search")
            .bearer_auth(&token.access_token)
            .query(&[("q", query), ("type", "track,album,playlist"), ("limit", "8")])
            .send()
            .await
            .map_err(|e| eyre!("Spotify search request failed: {e}"))?
            .error_for_status()
            .map_err(|e| eyre!("Spotify search failed: {e}"))?;
        response
            .text()
            .await
            .map_err(|e| eyre!("failed to read Spotify search response: {e}"))
    })?;
    parse_search_response(&body)
}
```

Add an ignored live-network smoke test at the top of the test module (same pattern as `test_play` in `tests/integration.rs`):

```rust
    // Live network + login required: cargo test search_smoke -- --ignored
    #[test]
    #[ignore]
    fn search_smoke() {
        let results = search("aphex twin").unwrap();
        assert!(!results.tracks.is_empty());
        assert!(results.tracks[0].uri.starts_with("spotify:track:"));
    }
```

`ctx()` and `session()` in `src/spotify/mod.rs` are private module functions; a child module reaches them via `super::`, so no visibility changes are needed.

- [ ] **Step 6: Build, test, and run the live smoke test**

Run: `cargo build && cargo test --lib spotify::search`
Expected: build clean, parser tests PASS.

Run: `cargo test search_smoke -- --ignored --nocapture`
Expected: PASS (requires being logged in: `looper spotify login`).

**If it fails with 401/403:** the token audience is wrong for the public API. Replace the `token_provider()` call with librespot's login5 token:

```rust
        let token = session
            .login5()
            .auth_token()
            .await
            .map_err(|e| eyre!("failed to get Spotify API token: {e}"))?;
```

(`token.access_token` field name is the same.) Re-run the smoke test; whichever variant passes is the one to keep.

- [ ] **Step 7: Commit**

```bash
git add src/spotify/search.rs src/spotify/mod.rs
git commit -m "feat(spotify): add catalog search via Web API"
```
(Include `Cargo.toml`/`Cargo.lock` in the `git add` only if Step 3's serde-derive note applied.)

---

### Task 2: Search panel state, flattening, and vim navigation helpers

**Files:**
- Modify: `src/tui.rs` — add types near `HistoryPanelState` (~line 78) and helpers + tests at the bottom (`mod tests` is ~line 1461)

**Interfaces:**
- Consumes: `crate::spotify::{SearchItem, SearchResults}` from Task 1.
- Produces (used by Tasks 3–5):
  - `pub struct SearchPanelState { pub input: String, pub focus: SearchFocus, pub entries: Vec<SearchEntry>, pub selected: usize, pub status: SearchStatus, pub pending_g: bool }`
  - `impl SearchPanelState { pub fn new() -> Self }` (empty input, `Query` focus, `Idle`, no entries)
  - `pub enum SearchFocus { Query, Results }`
  - `pub enum SearchStatus { Idle, Searching, Error(String) }` (derives `PartialEq` on the enum is NOT needed; match instead)
  - `pub enum SearchEntry { Header(&'static str), Item(SearchItem) }`
  - `pub fn flatten_results(results: SearchResults) -> Vec<SearchEntry>`
  - `pub fn next_item(entries: &[SearchEntry], from: usize) -> usize`
  - `pub fn prev_item(entries: &[SearchEntry], from: usize) -> usize`
  - `pub fn first_item(entries: &[SearchEntry]) -> Option<usize>`
  - `pub fn last_item(entries: &[SearchEntry]) -> Option<usize>`

- [ ] **Step 1: Write failing tests**

In `src/tui.rs`'s existing `#[cfg(test)] mod tests`, add:

```rust
    fn item(title: &str) -> crate::spotify::SearchItem {
        crate::spotify::SearchItem {
            title: title.into(),
            byline: String::new(),
            detail: String::new(),
            uri: format!("spotify:track:{title}"),
        }
    }

    fn sample_entries() -> Vec<SearchEntry> {
        vec![
            SearchEntry::Header("SONGS"),
            SearchEntry::Item(item("a")),
            SearchEntry::Item(item("b")),
            SearchEntry::Header("ALBUMS"),
            SearchEntry::Item(item("c")),
        ]
    }

    #[test]
    fn search_navigation_skips_headers() {
        let entries = sample_entries();
        assert_eq!(first_item(&entries), Some(1));
        assert_eq!(last_item(&entries), Some(4));
        assert_eq!(next_item(&entries, 1), 2);
        assert_eq!(next_item(&entries, 2), 4); // hops the ALBUMS header
        assert_eq!(next_item(&entries, 4), 4); // pinned at end
        assert_eq!(prev_item(&entries, 4), 2); // hops back over the header
        assert_eq!(prev_item(&entries, 1), 1); // pinned at start
    }

    #[test]
    fn flatten_skips_empty_sections() {
        let results = crate::spotify::SearchResults {
            tracks: vec![item("t")],
            albums: Vec::new(),
            playlists: vec![item("p")],
        };
        let entries = flatten_results(results);
        let headers: Vec<&str> = entries
            .iter()
            .filter_map(|e| match e {
                SearchEntry::Header(h) => Some(*h),
                _ => None,
            })
            .collect();
        assert_eq!(headers, vec!["SONGS", "PLAYLISTS"]);
        assert_eq!(entries.len(), 4);
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib tui::tests::search_navigation_skips_headers`
Expected: COMPILE ERROR — `SearchEntry` etc. not found.

- [ ] **Step 3: Implement state and helpers**

In `src/tui.rs`, directly below `HistoryPanelState` (~line 86), add:

```rust
/// In-TUI Spotify search overlay. Opened with `/` from the playback screen or
/// the history browser; closed with Esc.
pub struct SearchPanelState {
    pub input: String,
    pub focus: SearchFocus,
    /// Flattened section headers + selectable items, in render order.
    pub entries: Vec<SearchEntry>,
    /// Index into `entries`; always points at an `Item`, never a `Header`.
    pub selected: usize,
    pub status: SearchStatus,
    /// True after a first `g` in results focus, waiting for the second.
    pub pending_g: bool,
}

impl SearchPanelState {
    pub fn new() -> Self {
        SearchPanelState {
            input: String::new(),
            focus: SearchFocus::Query,
            entries: Vec::new(),
            selected: 0,
            status: SearchStatus::Idle,
            pending_g: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SearchFocus {
    Query,
    Results,
}

pub enum SearchStatus {
    Idle,
    Searching,
    Error(String),
}

pub enum SearchEntry {
    Header(&'static str),
    Item(crate::spotify::SearchItem),
}

pub fn flatten_results(results: crate::spotify::SearchResults) -> Vec<SearchEntry> {
    let mut entries = Vec::new();
    for (header, items) in [
        ("SONGS", results.tracks),
        ("ALBUMS", results.albums),
        ("PLAYLISTS", results.playlists),
    ] {
        if items.is_empty() {
            continue;
        }
        entries.push(SearchEntry::Header(header));
        entries.extend(items.into_iter().map(SearchEntry::Item));
    }
    entries
}

fn is_item(entry: &SearchEntry) -> bool {
    matches!(entry, SearchEntry::Item(_))
}

pub fn first_item(entries: &[SearchEntry]) -> Option<usize> {
    entries.iter().position(is_item)
}

pub fn last_item(entries: &[SearchEntry]) -> Option<usize> {
    entries.iter().rposition(is_item)
}

/// Next selectable index after `from`, skipping headers; pinned at the last item.
pub fn next_item(entries: &[SearchEntry], from: usize) -> usize {
    entries
        .iter()
        .enumerate()
        .skip(from + 1)
        .find(|(_, e)| is_item(e))
        .map(|(i, _)| i)
        .unwrap_or(from)
}

/// Previous selectable index before `from`, skipping headers; pinned at the first item.
pub fn prev_item(entries: &[SearchEntry], from: usize) -> usize {
    entries[..from]
        .iter()
        .rposition(is_item)
        .unwrap_or(from)
}
```

Also add the field to `AppState` (after `history_panel` at `src/tui.rs:48`):

```rust
    pub search_panel: Option<SearchPanelState>,
```

This breaks the two `AppState { .. }` construction sites; add `search_panel: None,` to both:
- `src/play_loop.rs:1289` (the real one in `play_tracks`)
- `src/play_loop.rs:1744` (the `base_state()` test helper)

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib`
Expected: new tests PASS, all existing tests still PASS (the two construction sites compile).

- [ ] **Step 5: Commit**

```bash
git add src/tui.rs src/play_loop.rs
git commit -m "feat(tui): add search panel state and vim navigation helpers"
```

---

### Task 3: Render the search overlay

**Files:**
- Modify: `src/tui.rs` — `draw()` (~line 155) and a new `draw_search_overlay` near `draw_history_panel` (~line 1215)

**Interfaces:**
- Consumes: `SearchPanelState`, `SearchEntry`, `SearchFocus`, `SearchStatus` from Task 2.
- Produces: `pub fn draw_search_overlay(frame: &mut ratatui::Frame, panel: &SearchPanelState)` — public so `browse_history_session` (Task 5) can call it too.

- [ ] **Step 1: Wire the overlay into `draw()`**

In `draw()` at `src/tui.rs:178`, after the history-panel block:

```rust
    if let Some(panel) = &state.search_panel {
        draw_search_overlay(frame, panel);
    }
```

(Search draws last: it must sit on top if both panels are somehow open.)

- [ ] **Step 2: Implement `draw_search_overlay`**

Add near `draw_history_panel`. Reuse the existing `centered_rect` helper and the history panel's palette:

```rust
pub fn draw_search_overlay(frame: &mut ratatui::Frame, panel: &SearchPanelState) {
    let area = centered_rect(72, 72, frame.area());
    frame.render_widget(Clear, area);

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(90, 90, 120)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // title + controls
            Constraint::Length(2), // query input
            Constraint::Min(0),    // results / status
        ])
        .split(inner);

    let controls = match panel.focus {
        SearchFocus::Query => "  •  type to edit  enter search  esc close",
        SearchFocus::Results => "  •  j/k move  gg/G top/bottom  / edit query  enter play  esc close",
    };
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![
            Span::styled(
                "Spotify Search",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(controls, Style::default().fg(Color::Rgb(150, 150, 170))),
        ])]),
        chunks[0],
    );

    let cursor = if panel.focus == SearchFocus::Query { "▏" } else { "" };
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![
            Span::styled("/ ", Style::default().fg(Color::Rgb(255, 180, 80))),
            Span::styled(
                format!("{}{cursor}", panel.input),
                Style::default().fg(Color::White),
            ),
        ])]),
        chunks[1],
    );

    match &panel.status {
        SearchStatus::Searching => {
            frame.render_widget(
                Paragraph::new(vec![Line::from(Span::styled(
                    "searching…",
                    Style::default().fg(Color::Rgb(180, 180, 200)),
                ))]),
                chunks[2],
            );
            return;
        }
        SearchStatus::Error(message) => {
            frame.render_widget(
                Paragraph::new(vec![Line::from(Span::styled(
                    message.clone(),
                    Style::default().fg(Color::Rgb(230, 130, 130)),
                ))]),
                chunks[2],
            );
            return;
        }
        SearchStatus::Idle => {}
    }

    let dim = Style::default().fg(Color::Rgb(170, 175, 200));
    let header_style = Style::default()
        .fg(Color::Rgb(255, 180, 80))
        .add_modifier(Modifier::BOLD);

    // Keep the selection visible: scroll so `selected` stays inside the
    // viewport (headers included in the row count).
    let height = chunks[2].height as usize;
    let skip = panel.selected.saturating_sub(height.saturating_sub(1));

    let lines: Vec<Line> = panel
        .entries
        .iter()
        .enumerate()
        .skip(skip)
        .take(height)
        .map(|(index, entry)| match entry {
            SearchEntry::Header(header) => Line::from(Span::styled(*header, header_style)),
            SearchEntry::Item(item) => {
                let marker = if index == panel.selected
                    && panel.focus == SearchFocus::Results
                {
                    "▸ "
                } else {
                    "  "
                };
                let row_style = if index == panel.selected
                    && panel.focus == SearchFocus::Results
                {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(210, 210, 225))
                };
                let mut spans = vec![Span::styled(
                    format!("{marker}{}", item.title),
                    row_style,
                )];
                if !item.byline.is_empty() {
                    spans.push(Span::styled(format!("  {}", item.byline), dim));
                }
                if !item.detail.is_empty() {
                    spans.push(Span::styled(format!("  {}", item.detail), dim));
                }
                Line::from(spans)
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), chunks[2]);
}
```

If `centered_rect` takes different parameters than `(percent_x, percent_y, rect)`, match whatever `draw_history_panel` at `src/tui.rs:1216` passes.

- [ ] **Step 3: Build and test**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass (rendering has no unit tests; visuals verified in Task 6's smoke test).

- [ ] **Step 4: Commit**

```bash
git add src/tui.rs
git commit -m "feat(tui): render Spotify search overlay"
```

---

### Task 4: Key routing, dispatch, and search execution in the playback loop

**Files:**
- Modify: `src/play_loop.rs` — `KeyCommand` (~line 852), `handle_key_event` (~line 875), `dispatch_command` (~line 564), `run_loop` (~line 465), `browse_history_session`'s exhaustive match arm (~line 188), plus tests in `mod tests` (~line 1739)

**Interfaces:**
- Consumes: everything from Tasks 1–3.
- Produces:
  - `KeyCommand` variants: `SearchOpen`, `SearchChar(char)`, `SearchBackspace`, `SearchSubmit`, `SearchNext`, `SearchPrev`, `SearchG`, `SearchBottom`, `SearchClose`, `SearchPlay`
  - `pub(crate) fn handle_search_key_event(key: KeyEvent, panel: &SearchPanelState) -> KeyCommand` (shared with Task 5)
  - `pub(crate) fn execute_search(panel: &mut SearchPanelState)` (shared with Task 5)

- [ ] **Step 1: Write failing key-routing tests**

In `src/play_loop.rs`'s `mod tests`, following the existing `base_state()` style:

```rust
    fn state_with_search() -> AppState {
        let mut state = base_state();
        state.search_panel = Some(SearchPanelState::new());
        state
    }

    #[test]
    fn slash_opens_search_from_playback() {
        let state = base_state();
        let cmd = handle_key_event(
            KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
            &state,
        );
        assert_eq!(cmd, KeyCommand::SearchOpen);
    }

    #[test]
    fn search_query_focus_captures_text() {
        let state = state_with_search();
        // 'q' must be text input, not Quit
        assert_eq!(
            handle_key_event(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE), &state),
            KeyCommand::SearchChar('q'),
        );
        assert_eq!(
            handle_key_event(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE), &state),
            KeyCommand::SearchBackspace,
        );
        assert_eq!(
            handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), &state),
            KeyCommand::SearchSubmit,
        );
        assert_eq!(
            handle_key_event(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE), &state),
            KeyCommand::SearchClose,
        );
    }

    #[test]
    fn search_results_focus_uses_vim_keys() {
        let mut state = state_with_search();
        let panel = state.search_panel.as_mut().unwrap();
        panel.focus = SearchFocus::Results;
        let state = state; // re-freeze
        for (code, expected) in [
            (KeyCode::Char('j'), KeyCommand::SearchNext),
            (KeyCode::Char('k'), KeyCommand::SearchPrev),
            (KeyCode::Char('g'), KeyCommand::SearchG),
            (KeyCode::Char('G'), KeyCommand::SearchBottom),
            (KeyCode::Char('/'), KeyCommand::SearchOpen),
            (KeyCode::Enter, KeyCommand::SearchPlay),
            (KeyCode::Esc, KeyCommand::SearchClose),
        ] {
            assert_eq!(
                handle_key_event(KeyEvent::new(code, KeyModifiers::NONE), &state),
                expected,
                "key {code:?}"
            );
        }
    }

    #[test]
    fn ctrl_c_still_quits_inside_search() {
        let state = state_with_search();
        assert_eq!(
            handle_key_event(
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
                &state
            ),
            KeyCommand::Quit,
        );
    }
```

Add the imports the tests need (`SearchFocus`, `SearchPanelState`) to the test module's `use` lines.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib play_loop`
Expected: COMPILE ERROR — `KeyCommand::SearchOpen` etc. not found.

- [ ] **Step 3: Implement key routing and dispatch**

3a. Extend `KeyCommand` (~line 852):

```rust
    SearchOpen,
    SearchChar(char),
    SearchBackspace,
    SearchSubmit,
    SearchNext,
    SearchPrev,
    SearchG,
    SearchBottom,
    SearchClose,
    SearchPlay,
```

3b. In `handle_key_event`, add the search branch FIRST (before the history-panel branch), and `/` bindings to both existing branches:

```rust
fn handle_key_event(key: KeyEvent, state: &AppState) -> KeyCommand {
    if let Some(panel) = &state.search_panel {
        return handle_search_key_event(key, panel);
    }
    // ... existing history-panel branch, with one added binding:
    //     (KeyCode::Char('/'), _) => KeyCommand::SearchOpen,
    // ... existing base branch, with one added binding:
    //     (KeyCode::Char('/'), _) => KeyCommand::SearchOpen,
```

Place the `/` arm in the history branch above the catch-all; in the base branch anywhere among the char bindings.

3c. Add the shared search key handler (near `handle_history_browser_key_event`):

```rust
/// Key routing while the search overlay is open. The overlay captures all
/// keys (so `q` can be typed and `j` never leaks to history navigation);
/// only Ctrl-C still quits.
pub(crate) fn handle_search_key_event(key: KeyEvent, panel: &SearchPanelState) -> KeyCommand {
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return KeyCommand::Quit;
    }
    match panel.focus {
        SearchFocus::Query => match key.code {
            KeyCode::Enter => KeyCommand::SearchSubmit,
            KeyCode::Esc => KeyCommand::SearchClose,
            KeyCode::Backspace => KeyCommand::SearchBackspace,
            KeyCode::Char(c) => KeyCommand::SearchChar(c),
            _ => KeyCommand::None,
        },
        SearchFocus::Results => match key.code {
            KeyCode::Char('j') | KeyCode::Down => KeyCommand::SearchNext,
            KeyCode::Char('k') | KeyCode::Up => KeyCommand::SearchPrev,
            KeyCode::Char('g') => KeyCommand::SearchG,
            KeyCode::Char('G') => KeyCommand::SearchBottom,
            KeyCode::Char('/') => KeyCommand::SearchOpen,
            KeyCode::Enter => KeyCommand::SearchPlay,
            KeyCode::Esc => KeyCommand::SearchClose,
            _ => KeyCommand::None,
        },
    }
}
```

3d. Add dispatch arms to `dispatch_command` (before `KeyCommand::None`):

```rust
        KeyCommand::SearchOpen => {
            match state.search_panel.as_mut() {
                // `/` while in results focus: back to editing the query.
                Some(panel) => {
                    panel.focus = SearchFocus::Query;
                    panel.pending_g = false;
                }
                None => state.search_panel = Some(SearchPanelState::new()),
            }
            *needs_render = true;
            Ok(None)
        }
        KeyCommand::SearchClose => {
            state.search_panel = None;
            *needs_render = true;
            Ok(None)
        }
        KeyCommand::SearchChar(c) => {
            if let Some(panel) = state.search_panel.as_mut() {
                panel.input.push(c);
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::SearchBackspace => {
            if let Some(panel) = state.search_panel.as_mut() {
                panel.input.pop();
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::SearchSubmit => {
            if let Some(panel) = state.search_panel.as_mut() {
                if !panel.input.trim().is_empty() {
                    // Render the "searching…" frame first; run_loop executes
                    // the blocking search on its next iteration.
                    panel.status = SearchStatus::Searching;
                    *needs_render = true;
                }
            }
            Ok(None)
        }
        KeyCommand::SearchNext => {
            if let Some(panel) = state.search_panel.as_mut() {
                panel.selected = next_item(&panel.entries, panel.selected);
                panel.pending_g = false;
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::SearchPrev => {
            if let Some(panel) = state.search_panel.as_mut() {
                panel.selected = prev_item(&panel.entries, panel.selected);
                panel.pending_g = false;
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::SearchG => {
            if let Some(panel) = state.search_panel.as_mut() {
                if panel.pending_g {
                    if let Some(first) = first_item(&panel.entries) {
                        panel.selected = first;
                    }
                    panel.pending_g = false;
                    *needs_render = true;
                } else {
                    panel.pending_g = true;
                }
            }
            Ok(None)
        }
        KeyCommand::SearchBottom => {
            if let Some(panel) = state.search_panel.as_mut() {
                if let Some(last) = last_item(&panel.entries) {
                    panel.selected = last;
                }
                panel.pending_g = false;
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::SearchPlay => {
            if let Some(panel) = &state.search_panel {
                if let Some(SearchEntry::Item(item)) = panel.entries.get(panel.selected) {
                    return Ok(Some(LoopAction::ReplayTarget(item.uri.clone())));
                }
            }
            Ok(None)
        }
```

Import the Task 2 helpers in `play_loop.rs`'s existing `use crate::tui::{...}` line: `flatten_results, first_item, last_item, next_item, prev_item, SearchEntry, SearchFocus, SearchPanelState, SearchStatus`.

3e. Add the shared search executor (near `toggle_history_panel`):

```rust
/// Run the blocking search and load results into the panel. Called after the
/// "searching…" frame has been drawn.
pub(crate) fn execute_search(panel: &mut SearchPanelState) {
    match crate::spotify::search(&panel.input) {
        Ok(results) => {
            panel.entries = flatten_results(results);
            match first_item(&panel.entries) {
                Some(first) => {
                    panel.selected = first;
                    panel.focus = SearchFocus::Results;
                    panel.status = SearchStatus::Idle;
                }
                None => {
                    panel.status =
                        SearchStatus::Error(format!("no results for \"{}\"", panel.input));
                    panel.focus = SearchFocus::Query;
                }
            }
        }
        Err(error) => {
            panel.status = SearchStatus::Error(error.to_string());
            panel.focus = SearchFocus::Query;
        }
    }
    panel.pending_g = false;
}
```

3f. Execute pending searches in `run_loop`. After the media-key drain (`while let Ok(cmd) = ctx.cmd_rx.try_recv() { ... }` block ending ~line 527), add:

```rust
        // A submitted search: draw the "searching…" frame, then block on the
        // Web API call (~300ms; audio is unaffected — rodio owns its thread).
        if state
            .search_panel
            .as_ref()
            .is_some_and(|p| matches!(p.status, SearchStatus::Searching))
        {
            state.frame_count += 1;
            terminal.draw(|f| draw(f, state))?;
            if let Some(panel) = state.search_panel.as_mut() {
                execute_search(panel);
            }
            needs_render = true;
        }
```

3g. `browse_history_session`'s exhaustive `match` (~line 188) now fails to compile. Add the new variants to its ignore-arm for now (Task 5 wires them properly):

```rust
                    KeyCommand::None
                    | KeyCommand::TogglePause
                    // ... existing ignored variants ...
                    | KeyCommand::SearchOpen
                    | KeyCommand::SearchChar(_)
                    | KeyCommand::SearchBackspace
                    | KeyCommand::SearchSubmit
                    | KeyCommand::SearchNext
                    | KeyCommand::SearchPrev
                    | KeyCommand::SearchG
                    | KeyCommand::SearchBottom
                    | KeyCommand::SearchClose
                    | KeyCommand::SearchPlay => {}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib`
Expected: all PASS, including the four new key-routing tests.

- [ ] **Step 5: Commit**

```bash
git add src/play_loop.rs src/tui.rs
git commit -m "feat(play): wire / search overlay into the playback loop"
```

---

### Task 5: Search from the history browser

**Files:**
- Modify: `src/play_loop.rs` — `browse_history_session` (~line 87) and `handle_history_browser_key_event` (~line 919), plus one test

**Interfaces:**
- Consumes: `handle_search_key_event`, `execute_search`, `SearchPanelState`, `draw_search_overlay` from Tasks 2–4.
- Produces: nothing new — behavior only.

- [ ] **Step 1: Write the failing test**

```rust
    #[test]
    fn slash_opens_search_from_history_browser() {
        let panel = HistoryPanelState {
            rows: Vec::new(),
            selected: 0,
            sort_field: HistorySortField::TimePlayed,
            descending: true,
            pending_delete: false,
        };
        assert_eq!(
            handle_history_browser_key_event(
                KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE),
                &panel
            ),
            KeyCommand::SearchOpen,
        );
    }
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cargo test --lib slash_opens_search_from_history_browser`
Expected: FAIL — `/` currently maps to `KeyCommand::None`.

- [ ] **Step 3: Wire the history browser**

3a. Add to `handle_history_browser_key_event`'s match (above the catch-all):

```rust
        (KeyCode::Char('/'), _) => KeyCommand::SearchOpen,
```

3b. In `browse_history_session`, add a local overlay state after the panel setup (~line 113):

```rust
    let mut search: Option<SearchPanelState> = None;
```

3c. Replace the draw call (~line 118) so the overlay renders on top:

```rust
        terminal.draw(|frame| {
            draw_history_browser(frame, &panel, sync_warning.as_ref());
            if let Some(search_panel) = &search {
                draw_search_overlay(frame, search_panel);
            }
        })?;
```

Import `draw_search_overlay` in the existing `use crate::tui::{...}` list.

3d. Route keys to the overlay when it is open. Replace the key-handling head of the loop body. Compute the command first so the `search` borrow ends before any arm reassigns `search` (holding `search.as_mut()` across the match would not borrow-check against `SearchClose`/`SearchPlay` setting `search = None`):

```rust
        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                if search.is_some() {
                    let cmd = handle_search_key_event(key, search.as_ref().expect("checked"));
                    match cmd {
                        KeyCommand::Quit => {
                            push_replica_best_effort(&storage);
                            return Ok(());
                        }
                        KeyCommand::SearchClose => search = None,
                        KeyCommand::SearchChar(c) => {
                            search.as_mut().expect("checked").input.push(c);
                        }
                        KeyCommand::SearchBackspace => {
                            search.as_mut().expect("checked").input.pop();
                        }
                        KeyCommand::SearchSubmit => {
                            let search_panel = search.as_mut().expect("checked");
                            if !search_panel.input.trim().is_empty() {
                                search_panel.status = SearchStatus::Searching;
                                terminal.draw(|frame| {
                                    draw_history_browser(frame, &panel, sync_warning.as_ref());
                                    draw_search_overlay(frame, search_panel);
                                })?;
                                execute_search(search_panel);
                            }
                        }
                        KeyCommand::SearchNext => {
                            let search_panel = search.as_mut().expect("checked");
                            search_panel.selected =
                                next_item(&search_panel.entries, search_panel.selected);
                            search_panel.pending_g = false;
                        }
                        KeyCommand::SearchPrev => {
                            let search_panel = search.as_mut().expect("checked");
                            search_panel.selected =
                                prev_item(&search_panel.entries, search_panel.selected);
                            search_panel.pending_g = false;
                        }
                        KeyCommand::SearchG => {
                            let search_panel = search.as_mut().expect("checked");
                            if search_panel.pending_g {
                                if let Some(first) = first_item(&search_panel.entries) {
                                    search_panel.selected = first;
                                }
                                search_panel.pending_g = false;
                            } else {
                                search_panel.pending_g = true;
                            }
                        }
                        KeyCommand::SearchBottom => {
                            let search_panel = search.as_mut().expect("checked");
                            if let Some(last) = last_item(&search_panel.entries) {
                                search_panel.selected = last;
                            }
                            search_panel.pending_g = false;
                        }
                        KeyCommand::SearchOpen => {
                            let search_panel = search.as_mut().expect("checked");
                            search_panel.focus = SearchFocus::Query;
                            search_panel.pending_g = false;
                        }
                        KeyCommand::SearchPlay => {
                            let target = {
                                let search_panel = search.as_ref().expect("checked");
                                match search_panel.entries.get(search_panel.selected) {
                                    Some(SearchEntry::Item(item)) => Some(item.uri.clone()),
                                    _ => None,
                                }
                            };
                            if let Some(target) = target {
                                search = None;
                                match play_file_session(
                                    terminal, title_state, &target, ctx, picker,
                                )? {
                                    SessionOutcome::Quit => {
                                        push_replica_best_effort(&storage);
                                        return Ok(());
                                    }
                                    SessionOutcome::BackToHistory => {
                                        refresh_history_panel(&mut panel, &storage)?;
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                    continue;
                }
                match handle_history_browser_key_event(key, &panel) {
                    KeyCommand::SearchOpen => {
                        search = Some(SearchPanelState::new());
                    }
                    // ... all existing arms unchanged ...
```

The `continue` after the search match is essential — while the overlay is open, no key reaches history navigation.

- [ ] **Step 4: Run all tests**

Run: `cargo test --lib`
Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add src/play_loop.rs
git commit -m "feat(history): open Spotify search with / from the history browser"
```

---

### Task 6: Docs and manual smoke test

**Files:**
- Modify: `CLAUDE.md` (TUI states section), `README.md` (keyboard shortcuts section, if one exists — check with `rg -n "shortcut|keys" README.md`)

**Interfaces:** none — documentation and verification only.

- [ ] **Step 1: Update docs**

In `CLAUDE.md`'s "TUI states" section, add a third UI mode entry:

```markdown
- search overlay (`/` from playback or the history browser)
  - Spotify catalog search via the Web API (token minted from the librespot
    session — Premium login required, no yt-dlp)
  - query focus: type, Enter searches, Esc closes
  - results focus: j/k move, gg/G top/bottom, `/` edits the query,
    Enter plays the selection through the normal replay/resolve path
```

Add `src/spotify/search.rs` to the "Spotify playback model" module list. Update README key documentation if present.

- [ ] **Step 2: Full build and test**

Run: `cargo build && cargo test`
Expected: clean build, all tests pass.

- [ ] **Step 3: Manual smoke test (needs a terminal + audio + Spotify Premium login)**

```bash
cargo run -- play --url tests/fixtures/sound.mp3
```

Checklist:
1. `/` opens the overlay; audio keeps playing.
2. Type `q` — it appears in the query (does not quit). Backspace edits.
3. Search `aphex twin` — "searching…" flashes, sectioned results appear.
4. `j`/`k` skip section headers; `gg`/`G` jump; `/` returns to query editing.
5. Enter on a SONG — playback swaps, track loops, history records it.
6. `/` again, Enter on an ALBUM — playlist semantics (tracks advance, `n`/`b` work).
7. `/`, Enter on a PLAYLIST — same.
8. `p` opens history browser path: quit, run `looper play --url <any spotify url>`, press `p`, then `/` — search works from the history panel too.
9. Esc closes the overlay cleanly; `q` afterwards quits; terminal restores (no raw-mode residue).
10. Log-out simulation: temporarily rename the Spotify cache credentials file and search — overlay shows the login-hint error, playback unaffected.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md README.md
git commit -m "docs: document Spotify search overlay"
```
