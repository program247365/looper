//! Spotify catalog search via the public Web API.
//!
//! Playback and metadata go through librespot's own protocols, but librespot
//! exposes no search, and Spotify rejects Web API calls made with tokens from
//! librespot's shared client id (every endpoint answers 429; the Mercury
//! keymaster and searchview routes are retired outright). Search therefore
//! needs the user's own — free — Spotify API app: `SPOTIFY_CLIENT_ID` and
//! `SPOTIFY_CLIENT_SECRET` feed a client-credentials token (no browser flow,
//! no user context), cached in-process for its ~1h lifetime.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use color_eyre::eyre::{eyre, Result, WrapErr};
use serde::Deserialize;

/// Display limits per section (8 tracks, 3 artists, 8 albums, 5 playlists).
const TRACK_LIMIT: usize = 8;
const ARTIST_LIMIT: usize = 3;
const ALBUM_LIMIT: usize = 8;
const PLAYLIST_LIMIT: usize = 5;

/// Spotify rejects `limit` above 10 for API apps in development mode (400
/// "Invalid limit" since the 2025 Web API restrictions), so both search and
/// discography paging are pinned to 10.
const API_PAGE_LIMIT: usize = 10;

/// Upper bound on discography entries fetched (5 pages). Bounds the blocking
/// time on the TUI thread; prolific artists lose the tail of their singles.
const DISCOGRAPHY_CAP: usize = 50;

#[derive(Debug, Default)]
pub struct SearchResults {
    pub tracks: Vec<SearchItem>,
    pub artists: Vec<SearchItem>,
    pub albums: Vec<SearchItem>,
    pub playlists: Vec<SearchItem>,
}

/// An artist's releases from `/v1/artists/{id}/albums`, grouped the way the
/// API tags them. Every item's `uri` is an album — a valid `resolve` target.
#[derive(Debug, Default)]
pub struct Discography {
    pub albums: Vec<SearchItem>,
    pub singles: Vec<SearchItem>,
    pub compilations: Vec<SearchItem>,
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

/// Shown in the search overlay when the API app env vars are missing.
const SETUP_HELP: &str = "Spotify search needs your own (free) Spotify API app:\n  1. create an app at developer.spotify.com/dashboard\n  2. export SPOTIFY_CLIENT_ID=... and SPOTIFY_CLIENT_SECRET=...\n  3. restart looper\nPlayback is unaffected — see the README's \"Spotify search\" section.";

/// Search Spotify's catalog. Blocking (~300ms), called from the TUI thread
/// after a "searching…" frame has been drawn. Requires `SPOTIFY_CLIENT_ID` /
/// `SPOTIFY_CLIENT_SECRET`; does NOT require the librespot Premium login.
pub fn search(query: &str) -> Result<SearchResults> {
    let token = search_token()?;
    let limit = API_PAGE_LIMIT.to_string();
    let response = reqwest::blocking::Client::new()
        .get("https://api.spotify.com/v1/search")
        .bearer_auth(&token)
        .query(&[
            ("q", query),
            ("type", "track,artist,album,playlist"),
            ("limit", &limit),
        ])
        .send()
        .map_err(|e| eyre!("Spotify search request failed: {e}"))?
        .error_for_status()
        .map_err(|e| eyre!("Spotify search failed: {e}"))?;
    let body = response
        .text()
        .map_err(|e| eyre!("failed to read Spotify search response: {e}"))?;
    parse_search_response(&body)
}

pub fn is_artist_uri(uri: &str) -> bool {
    uri.starts_with("spotify:artist:")
}

/// Fetch an artist's full discography, paging `/v1/artists/{id}/albums` at the
/// API max of [`API_PAGE_LIMIT`]. Blocking like [`search`] and called from the
/// same "searching…" rail in the TUI; worst case [`DISCOGRAPHY_CAP`]/10 calls.
pub fn artist_albums(artist_uri: &str) -> Result<Discography> {
    let id = artist_uri
        .strip_prefix("spotify:artist:")
        .ok_or_else(|| eyre!("not a Spotify artist URI: {artist_uri}"))?;
    let token = search_token()?;
    let client = reqwest::blocking::Client::new();
    let mut discography = Discography::default();
    let mut offset = 0;
    loop {
        let body = client
            .get(format!("https://api.spotify.com/v1/artists/{id}/albums"))
            .bearer_auth(&token)
            .query(&[
                ("include_groups", "album,single,compilation"),
                ("limit", &API_PAGE_LIMIT.to_string()),
                ("offset", &offset.to_string()),
            ])
            .send()
            .map_err(|e| eyre!("Spotify albums request failed: {e}"))?
            .error_for_status()
            .map_err(|e| eyre!("Spotify albums lookup failed: {e}"))?
            .text()
            .map_err(|e| eyre!("failed to read Spotify albums response: {e}"))?;
        let (page, total) = parse_artist_albums_page(&body)?;
        discography.albums.extend(page.albums);
        discography.singles.extend(page.singles);
        discography.compilations.extend(page.compilations);
        offset += API_PAGE_LIMIT;
        if offset >= total as usize || offset >= DISCOGRAPHY_CAP {
            break;
        }
    }
    Ok(discography)
}

struct CachedToken {
    access_token: String,
    expires_at: Instant,
}

static TOKEN: Mutex<Option<CachedToken>> = Mutex::new(None);

/// A valid client-credentials bearer token, fetched with the user's API app
/// and reused until shortly before expiry (~1h).
fn search_token() -> Result<String> {
    let mut slot = TOKEN.lock().unwrap();
    if let Some(cached) = slot.as_ref() {
        if Instant::now() < cached.expires_at {
            return Ok(cached.access_token.clone());
        }
    }

    let (client_id, client_secret) = search_app_credentials(
        std::env::var("SPOTIFY_CLIENT_ID").ok(),
        std::env::var("SPOTIFY_CLIENT_SECRET").ok(),
    )?;

    #[derive(Deserialize)]
    struct TokenResponse {
        access_token: String,
        expires_in: u64,
    }

    let body = reqwest::blocking::Client::new()
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(&client_id, Some(&client_secret))
        .form(&[("grant_type", "client_credentials")])
        .send()
        .map_err(|e| eyre!("Spotify token request failed: {e}"))?
        .error_for_status()
        .map_err(|e| {
            eyre!("Spotify rejected your API app credentials ({e}) — check SPOTIFY_CLIENT_ID / SPOTIFY_CLIENT_SECRET")
        })?
        .text()
        .map_err(|e| eyre!("failed to read Spotify token response: {e}"))?;
    let response: TokenResponse =
        serde_json::from_str(&body).wrap_err("unexpected Spotify token response")?;

    // Refresh a minute early so an in-flight search never carries a token
    // that expires mid-request.
    let expires_at =
        Instant::now() + Duration::from_secs(response.expires_in.saturating_sub(60).max(60));
    let token = response.access_token.clone();
    *slot = Some(CachedToken {
        access_token: response.access_token,
        expires_at,
    });
    Ok(token)
}

/// Validate the env-var pair, pointing at the setup instructions when absent.
/// Split out from `search_token` so the guidance path is testable without
/// touching process-global env state.
fn search_app_credentials(
    client_id: Option<String>,
    client_secret: Option<String>,
) -> Result<(String, String)> {
    match (client_id, client_secret) {
        (Some(id), Some(secret)) if !id.trim().is_empty() && !secret.trim().is_empty() => {
            Ok((id, secret))
        }
        _ => Err(eyre!("{SETUP_HELP}")),
    }
}

#[derive(Deserialize)]
struct ApiResponse {
    #[serde(default)]
    tracks: ApiPage<ApiTrack>,
    #[serde(default)]
    artists: ApiPage<ApiArtistResult>,
    #[serde(default)]
    albums: ApiPage<ApiAlbum>,
    #[serde(default)]
    playlists: ApiPage<ApiPlaylist>,
}

/// A paging object. Items are `Option` because the playlist search returns
/// literal nulls for entries Spotify no longer exposes.
#[derive(Deserialize)]
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

/// An artist as a search result of its own (`type=artist`), unlike
/// [`ApiArtist`] which is the credit line embedded in tracks/albums.
#[derive(Deserialize)]
struct ApiArtistResult {
    name: String,
    uri: String,
}

/// One page of `/v1/artists/{id}/albums`.
#[derive(Deserialize)]
struct ApiAlbumsPage {
    total: u64,
    items: Vec<Option<ApiArtistAlbum>>,
}

#[derive(Deserialize)]
struct ApiArtistAlbum {
    name: String,
    album_type: Option<String>,
    total_tracks: u64,
    release_date: Option<String>,
    #[serde(default)]
    artists: Vec<ApiArtist>,
    uri: String,
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

    let artists = response
        .artists
        .items
        .into_iter()
        .flatten()
        .take(ARTIST_LIMIT)
        .map(|a| SearchItem {
            title: a.name,
            byline: String::new(),
            detail: String::new(),
            uri: a.uri,
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
            byline: p.owner.and_then(|o| o.display_name).unwrap_or_default(),
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
        artists,
        albums,
        playlists,
    })
}

fn parse_artist_albums_page(body: &str) -> Result<(Discography, u64)> {
    let page: ApiAlbumsPage =
        serde_json::from_str(body).wrap_err("unexpected Spotify albums response")?;
    let mut discography = Discography::default();
    for album in page.items.into_iter().flatten() {
        let tracks = format!("{} tracks", album.total_tracks);
        let detail = match album.release_date.as_deref().and_then(|d| d.get(..4)) {
            Some(year) => format!("{year} · {tracks}"),
            None => tracks,
        };
        let item = SearchItem {
            byline: join_artists(&album.artists),
            detail,
            title: album.name,
            uri: album.uri,
        };
        match album.album_type.as_deref() {
            Some("single") => discography.singles.push(item),
            Some("compilation") => discography.compilations.push(item),
            _ => discography.albums.push(item),
        }
    }
    Ok((discography, page.total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_credentials_point_at_setup_docs() {
        let err = search_app_credentials(None, None).unwrap_err();
        assert!(err.to_string().contains("developer.spotify.com"));
        assert!(err.to_string().contains("SPOTIFY_CLIENT_ID"));
        // A set-but-empty variable gets the same guidance.
        let err = search_app_credentials(Some("id".into()), Some("  ".into())).unwrap_err();
        assert!(err.to_string().contains("developer.spotify.com"));
    }

    // Live network + SPOTIFY_CLIENT_ID/SPOTIFY_CLIENT_SECRET required:
    // cargo test search_smoke -- --ignored
    #[test]
    #[ignore]
    fn search_smoke() {
        let results = search("aphex twin").unwrap();
        assert!(!results.tracks.is_empty());
        assert!(results.tracks[0].uri.starts_with("spotify:track:"));
    }

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
    fn parses_artist_results() {
        let json = r#"{
            "artists": { "items": [
                { "name": "The Toxic Avenger", "uri": "spotify:artist:5zExRf0VQCl3GO4Jrj8r0s" },
                null
            ] }
        }"#;
        let results = parse_search_response(json).unwrap();
        assert_eq!(
            results.artists,
            vec![SearchItem {
                title: "The Toxic Avenger".into(),
                byline: String::new(),
                detail: String::new(),
                uri: "spotify:artist:5zExRf0VQCl3GO4Jrj8r0s".into(),
            }]
        );
    }

    #[test]
    fn recognizes_artist_uris() {
        assert!(is_artist_uri("spotify:artist:5zExRf0VQCl3GO4Jrj8r0s"));
        assert!(!is_artist_uri("spotify:album:1WuUwNAeBHEIxdXK2mmzvL"));
        assert!(!is_artist_uri("spotify:track:5MMWpTWKyTUottUuQxRXVx"));
    }

    #[test]
    fn parses_artist_albums_page_into_groups() {
        let json = r#"{
            "total": 3,
            "items": [
                { "name": "Globe, Vol. 3", "album_type": "album", "total_tracks": 8,
                  "release_date": "2017-05-12",
                  "artists": [{ "name": "The Toxic Avenger" }],
                  "uri": "spotify:album:globe3" },
                { "name": "Getting Started", "album_type": "single", "total_tracks": 2,
                  "release_date": "2019",
                  "artists": [{ "name": "The Toxic Avenger" }],
                  "uri": "spotify:album:gettingstarted" },
                { "name": "TXC RMX", "album_type": "compilation", "total_tracks": 12,
                  "release_date": "2015-03-01",
                  "artists": [{ "name": "The Toxic Avenger" }],
                  "uri": "spotify:album:txcrmx" }
            ]
        }"#;
        let (page, total) = parse_artist_albums_page(json).unwrap();
        assert_eq!(total, 3);
        assert_eq!(
            page.albums,
            vec![SearchItem {
                title: "Globe, Vol. 3".into(),
                byline: "The Toxic Avenger".into(),
                detail: "2017 · 8 tracks".into(),
                uri: "spotify:album:globe3".into(),
            }]
        );
        assert_eq!(page.singles.len(), 1);
        assert_eq!(page.singles[0].detail, "2019 · 2 tracks");
        assert_eq!(page.compilations.len(), 1);
        assert_eq!(page.compilations[0].title, "TXC RMX");
    }

    #[test]
    fn artist_albums_page_tolerates_missing_fields() {
        // An unknown album_type lands in `albums`; a missing release date
        // leaves just the track count.
        let json = r#"{
            "total": 1,
            "items": [
                { "name": "Mystery", "total_tracks": 5, "artists": [],
                  "uri": "spotify:album:mystery" }
            ]
        }"#;
        let (page, _) = parse_artist_albums_page(json).unwrap();
        assert_eq!(page.albums[0].detail, "5 tracks");
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

    #[test]
    fn truncates_albums_and_artists_to_display_limits() {
        let many_albums: Vec<String> = (0..10)
            .map(|i| format!(
                r#"{{ "name": "A{i}", "total_tracks": 1, "artists": [], "uri": "spotify:album:{i}" }}"#
            ))
            .collect();
        let many_artists: Vec<String> = (0..10)
            .map(|i| format!(r#"{{ "name": "R{i}", "uri": "spotify:artist:{i}" }}"#))
            .collect();
        let json = format!(
            r#"{{ "albums": {{ "items": [{}] }}, "artists": {{ "items": [{}] }} }}"#,
            many_albums.join(","),
            many_artists.join(",")
        );
        let results = parse_search_response(&json).unwrap();
        assert_eq!(results.albums.len(), 8);
        assert_eq!(results.artists.len(), 3);
    }

    // Live network + SPOTIFY_CLIENT_ID/SPOTIFY_CLIENT_SECRET required:
    // cargo test discography_smoke -- --ignored
    #[test]
    #[ignore]
    fn discography_smoke() {
        // The Toxic Avenger (FR electro) — 14 albums as of 2026.
        let discography = artist_albums("spotify:artist:5zExRf0VQCl3GO4Jrj8r0s").unwrap();
        assert!(discography.albums.len() > 10);
        assert!(discography.albums[0].uri.starts_with("spotify:album:"));
    }
}
