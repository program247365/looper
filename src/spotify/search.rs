//! Spotify catalog search via the public Web API.
//!
//! Playback and metadata go through librespot's own protocols, but librespot
//! exposes no search. The Web API's `/v1/search` does, and the librespot
//! session can mint a bearer token for it — so search needs no second login
//! and no developer app.

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

/// Search Spotify's catalog. Blocking: runs one Web API request on the shared
/// Spotify runtime (~300ms). Requires a prior `looper spotify login`.
pub fn search(query: &str) -> Result<SearchResults> {
    let ctx = super::ctx()?;
    let session = super::session()?;
    let body = ctx.runtime.block_on(async move {
        let token = session
            .token_provider()
            .get_token("user-read-private,playlist-read-private")
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

    // Live network + login required: cargo test search_smoke -- --ignored
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
