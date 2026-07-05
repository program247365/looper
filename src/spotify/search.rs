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

/// Shown in the search overlay when the API app env vars are missing.
const SETUP_HELP: &str = "Spotify search needs your own (free) Spotify API app:\n  1. create an app at developer.spotify.com/dashboard\n  2. export SPOTIFY_CLIENT_ID=... and SPOTIFY_CLIENT_SECRET=...\n  3. restart looper\nPlayback is unaffected — see the README's \"Spotify search\" section.";

/// Search Spotify's catalog. Blocking (~300ms), called from the TUI thread
/// after a "searching…" frame has been drawn. Requires `SPOTIFY_CLIENT_ID` /
/// `SPOTIFY_CLIENT_SECRET`; does NOT require the librespot Premium login.
pub fn search(query: &str) -> Result<SearchResults> {
    let token = search_token()?;
    let response = reqwest::blocking::Client::new()
        .get("https://api.spotify.com/v1/search")
        .bearer_auth(&token)
        .query(&[("q", query), ("type", "track,album,playlist"), ("limit", "8")])
        .send()
        .map_err(|e| eyre!("Spotify search request failed: {e}"))?
        .error_for_status()
        .map_err(|e| eyre!("Spotify search failed: {e}"))?;
    let body = response
        .text()
        .map_err(|e| eyre!("failed to read Spotify search response: {e}"))?;
    parse_search_response(&body)
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
        albums,
        playlists,
    })
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
