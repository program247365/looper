//! Spotify playback via librespot.
//!
//! Spotify exposes no downloadable audio, so unlike the yt-dlp-backed services
//! we can't cache an MP3 and play it as a `File`. The only way to play a full
//! track is librespot, which authenticates as a Spotify Connect device
//! (Premium required) and decodes the DRM Ogg/Vorbis stream in-process. The
//! decoded PCM is bridged into rodio by [`sink`].
//!
//! A single authenticated [`Session`] is shared (lazily initialised) between
//! metadata resolution and playback. Credentials come from an OAuth login run
//! once via `looper spotify login`.

mod search;
mod sink;

pub use search::{search, SearchItem, SearchResults};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};
use stream_download::http::reqwest::Client as HttpClient;

use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use librespot_core::spotify_uri::SpotifyUri;
use librespot_metadata::audio::AudioItem;
use librespot_metadata::{Album, Metadata, Playlist, Track};
use librespot_oauth::OAuthClientBuilder;
use librespot_playback::config::PlayerConfig;
use librespot_playback::mixer::NoOpVolume;
use librespot_playback::player::{Player, PlayerEvent};
use tokio::runtime::Runtime;

use crate::playback_input::PlaybackInput;
use crate::plugin::TrackInfo;

use sink::{SpotifySource, SPOTIFY_CHANNELS, SPOTIFY_SAMPLE_RATE};

/// Loopback address librespot's OAuth flow redirects to after browser consent.
const OAUTH_REDIRECT_URI: &str = "http://127.0.0.1:8898/login";
/// 1 GiB on-disk cache of encrypted audio chunks. Looping a single track then
/// replays from disk instead of re-fetching from the network each loop.
const AUDIO_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

/// True for both share URLs (`https://open.spotify.com/...`) and `spotify:` URIs.
pub fn is_spotify_url(url: &str) -> bool {
    let url = url.trim();
    url.starts_with("spotify:") || url.contains("open.spotify.com")
}

/// Run the one-time OAuth browser flow and cache reusable credentials.
pub fn login() -> Result<()> {
    let config = SessionConfig::default();
    println!("Opening your browser to authorize looper with Spotify...");
    let token = OAuthClientBuilder::new(&config.client_id, OAUTH_REDIRECT_URI, vec!["streaming"])
        .open_in_browser()
        .build()
        .map_err(|e| eyre!("failed to start Spotify OAuth flow: {e}"))?
        .get_access_token()
        .map_err(|e| eyre!("Spotify OAuth failed: {e}"))?;

    let cache = open_cache()?;
    let runtime = build_runtime()?;
    // `Session::new` calls `Handle::current()`, so it must run inside the
    // runtime. store_credentials = true persists reusable credentials into the
    // cache so future runs skip the browser flow.
    let session = runtime
        .block_on(async move {
            let session = Session::new(config, Some(cache));
            session
                .connect(Credentials::with_access_token(token.access_token), true)
                .await
                .map(|()| session)
        })
        .map_err(|e| eyre!("Spotify login failed (Premium required): {e}"))?;

    println!(
        "Logged in to Spotify as {}. Credentials cached — you won't need to do this again.",
        session.username()
    );
    Ok(())
}

/// Resolve a Spotify URL/URI to playable tracks: a single track, or every
/// track of a playlist or album.
pub fn resolve(url: &str) -> Result<Vec<TrackInfo>> {
    let uri = parse_uri(url)?;
    let ctx = ctx()?;
    let session = session()?;
    match &uri {
        SpotifyUri::Track { .. } => {
            let info = ctx.runtime.block_on(async {
                let track = Track::get(&session, &uri)
                    .await
                    .map_err(|e| eyre!("failed to fetch Spotify track metadata: {e}"))?;
                let track_uri = uri
                    .to_uri()
                    .map_err(|e| eyre!("failed to build Spotify track URI: {e}"))?;
                ensure_track_available(&session, &uri).await?;
                let thumbnail = download_cover(&HttpClient::new(), &track, &art_dir()?, 0).await;
                Ok::<_, color_eyre::eyre::Report>(track_info(track, track_uri, None, thumbnail))
            })?;
            Ok(vec![info])
        }
        SpotifyUri::Playlist { .. } => {
            let tracks = ctx.runtime.block_on(async {
                let playlist = Playlist::get(&session, &uri)
                    .await
                    .map_err(|e| eyre!("failed to fetch Spotify playlist: {e}"))?;
                let name = Some(playlist.name().to_string());
                let uris: Vec<SpotifyUri> = playlist.tracks().cloned().collect();
                fetch_track_infos(&session, uris, name).await
            })?;
            ensure_nonempty(tracks, "playlist")
        }
        SpotifyUri::Album { .. } => {
            let tracks = ctx.runtime.block_on(async {
                let album = Album::get(&session, &uri)
                    .await
                    .map_err(|e| eyre!("failed to fetch Spotify album: {e}"))?;
                let name = Some(album.name.clone());
                let uris: Vec<SpotifyUri> = album.tracks().cloned().collect();
                fetch_track_infos(&session, uris, name).await
            })?;
            ensure_nonempty(tracks, "album")
        }
        _ => Err(eyre!(
            "unsupported Spotify link — pass a track, playlist, or album URL"
        )),
    }
}

/// Build a `TrackInfo` from track metadata and its canonical URI. The URI is
/// both the playback target and the history identity (it's a valid replay URL).
/// `collection` is the playlist/album name this track came from, if any;
/// `thumbnail_path` is its downloaded album cover, if any.
fn track_info(
    track: Track,
    track_uri: String,
    collection: Option<String>,
    thumbnail_path: Option<PathBuf>,
) -> TrackInfo {
    let artist = if track.artists.is_empty() {
        None
    } else {
        Some(
            track
                .artists
                .iter()
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
        )
    };
    TrackInfo {
        title: track.name,
        duration_secs: Some((track.duration.max(0) as f64) / 1000.0),
        playback: PlaybackInput::spotify(track_uri.clone()),
        source_url: Some(track_uri),
        pending_download: None,
        service: Some("Spotify".to_string()),
        thumbnail_path,
        is_live: false,
        collection,
        artist,
    }
}

/// Fetch metadata (and album art) for many track URIs, preserving order. Runs
/// in bounded concurrent batches so a large playlist resolves quickly without
/// firing hundreds of simultaneous requests. Tracks that fail to fetch are
/// dropped. `collection` (the playlist/album name) is stamped onto every track.
async fn fetch_track_infos(
    session: &Session,
    uris: Vec<SpotifyUri>,
    collection: Option<String>,
) -> Result<Vec<TrackInfo>> {
    const CONCURRENCY: usize = 16;
    let client = HttpClient::new();
    let art = art_dir()?;
    let mut infos: Vec<Option<TrackInfo>> = (0..uris.len()).map(|_| None).collect();
    let mut base = 0;
    for chunk in uris.chunks(CONCURRENCY) {
        let mut set = tokio::task::JoinSet::new();
        for (offset, uri) in chunk.iter().enumerate() {
            let session = session.clone();
            let client = client.clone();
            let art = art.clone();
            let collection = collection.clone();
            let uri = uri.clone();
            let position = base + offset;
            set.spawn(async move {
                let info = match Track::get(&session, &uri).await {
                    Ok(track) => match uri.to_uri() {
                        Ok(track_uri) => {
                            let thumbnail = download_cover(&client, &track, &art, position).await;
                            Some(track_info(track, track_uri, collection, thumbnail))
                        }
                        Err(_) => None,
                    },
                    Err(_) => None,
                };
                (position, info)
            });
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((position, Some(info))) = joined {
                infos[position] = Some(info);
            }
        }
        base += chunk.len();
    }
    Ok(infos.into_iter().flatten().collect())
}

/// Download a track's album cover (the largest size) from Spotify's public
/// image CDN into the art cache, returning its path. Covers are keyed by file
/// ID, so tracks sharing an album reuse one cached file. Best-effort: any
/// failure yields `None` (art is optional).
async fn download_cover(
    client: &HttpClient,
    track: &Track,
    art_dir: &Path,
    temp_tag: usize,
) -> Option<PathBuf> {
    let cover = track.album.covers.iter().max_by_key(|image| image.width)?;
    let hex = cover.id.to_base16().ok()?;
    let target = art_dir.join(format!("{hex}.jpg"));
    if target.exists() {
        return Some(target);
    }
    let url = format!("https://i.scdn.co/image/{hex}");
    let bytes = client
        .get(url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .await
        .ok()?;
    // Write to a per-task temp file then atomically rename, so concurrent
    // downloads of the same shared album cover can't corrupt the target.
    let temp = art_dir.join(format!("{hex}.{temp_tag}.tmp"));
    std::fs::write(&temp, &bytes).ok()?;
    std::fs::rename(&temp, &target).ok()?;
    Some(target)
}

/// Error if a track won't play for this account. librespot computes
/// availability against the account's country and follows relinked
/// alternatives, so this matches the player's own verdict: unavailable with no
/// playable alternative. Surfacing it as an error lets the caller show the
/// "track unavailable" modal instead of playing silence. Used only for a single
/// directly-requested track; playlist/album tracks are dropped at resolve.
async fn ensure_track_available(session: &Session, uri: &SpotifyUri) -> Result<()> {
    let item = AudioItem::get_file(session, uri.clone()).await.map_err(|_| {
        eyre!("this Spotify track couldn't be loaded (it may be removed or region-locked)")
    })?;
    let has_alternative = item
        .alternatives
        .as_ref()
        .is_some_and(|alts| !alts.is_empty());
    if item.availability.is_err() && !has_alternative {
        return Err(eyre!(
            "this Spotify track is unavailable (removed or region-locked)"
        ));
    }
    Ok(())
}

fn art_dir() -> Result<PathBuf> {
    let dir = crate::plugin::cache_dir_path()?.join("spotify").join("art");
    std::fs::create_dir_all(&dir).wrap_err("failed to create Spotify art cache directory")?;
    Ok(dir)
}

fn ensure_nonempty(tracks: Vec<TrackInfo>, kind: &str) -> Result<Vec<TrackInfo>> {
    if tracks.is_empty() {
        Err(eyre!("Spotify {kind} has no playable tracks"))
    } else {
        Ok(tracks)
    }
}

/// Active librespot playback. Dropping it stops the track and tears down the
/// end-of-track loop listener, releasing the `Player` it kept alive.
pub struct SpotifyPlayback {
    player: Arc<Player>,
    listener: tokio::task::AbortHandle,
}

impl Drop for SpotifyPlayback {
    fn drop(&mut self) {
        self.player.stop();
        self.listener.abort();
    }
}

/// Start playing a track. Returns the rodio source to append, a handle that
/// keeps playback alive, and the stream's PCM format + track duration.
pub fn open_playback(
    track_uri: &str,
    repeat: bool,
) -> Result<(SpotifySource, SpotifyPlayback, u32, u16, Option<Duration>)> {
    let ctx = ctx()?;
    let session = session()?;
    let uri =
        SpotifyUri::from_uri(track_uri).map_err(|e| eyre!("invalid Spotify track URI: {e}"))?;
    let (spotify_sink, source, end_signal) = sink::bridge();

    let (player, duration, listener) = ctx.runtime.block_on(async move {
        let duration = match Track::get(&session, &uri).await {
            Ok(track) => Some(Duration::from_millis(track.duration.max(0) as u64)),
            Err(_) => None,
        };

        let player = Player::new(
            PlayerConfig::default(),
            session,
            Box::new(NoOpVolume),
            move || Box::new(spotify_sink),
        );

        let mut events = player.get_player_event_channel();
        player.load(uri.clone(), true, 0);

        // On end-of-track: in single-track mode (`repeat`) re-load the same
        // track to loop it — the bridge emits silence during the brief reload
        // gap so audio never stops. In playlist mode, signal the source to end
        // so the sink empties and play_loop advances to the next track.
        let listener = {
            let player = player.clone();
            let loop_uri = uri.clone();
            tokio::spawn(async move {
                while let Some(event) = events.recv().await {
                    if matches!(event, PlayerEvent::EndOfTrack { .. }) {
                        if repeat {
                            player.load(loop_uri.clone(), true, 0);
                        } else {
                            end_signal.finish();
                            break;
                        }
                    }
                }
            })
            .abort_handle()
        };

        (player, duration, listener)
    });

    Ok((
        source,
        SpotifyPlayback { player, listener },
        SPOTIFY_SAMPLE_RATE,
        SPOTIFY_CHANNELS,
        duration,
    ))
}

/// Shared librespot runtime plus a session that can be rebuilt on demand. The
/// runtime is created once and never replaced — it hosts every track's player
/// tasks, so dropping it would kill playback. Only the session is swapped when
/// its connection dies.
struct SpotifyCtx {
    runtime: Runtime,
    session: Mutex<Option<Session>>,
}

static CTX: OnceLock<SpotifyCtx> = OnceLock::new();

/// The shared runtime + session slot, created once per process. Does not
/// connect — that happens lazily in [`session`].
fn ctx() -> Result<&'static SpotifyCtx> {
    if let Some(ctx) = CTX.get() {
        return Ok(ctx);
    }
    let ctx = SpotifyCtx {
        runtime: build_runtime()?,
        session: Mutex::new(None),
    };
    // A racing thread may have set it first; that's fine, drop ours.
    let _ = CTX.set(ctx);
    Ok(CTX.get().expect("context was just set"))
}

/// A connected librespot session (a cheap `Arc` clone). Reconnects from cached
/// credentials if the previous session died — e.g. after sleep/wake or a
/// network change — so playback recovers at the next track instead of failing.
/// librespot's core `Session` does not auto-reconnect, so we do it here. Called
/// at each track boundary (resolve / open_playback), never inside an async
/// context, so the blocking reconnect is safe.
fn session() -> Result<Session> {
    let ctx = ctx()?;
    let mut slot = ctx.session.lock().unwrap();
    let needs_connect = slot.as_ref().map_or(true, |session| session.is_invalid());
    if needs_connect {
        *slot = Some(connect_session(&ctx.runtime)?);
    }
    Ok(slot.as_ref().expect("session connected above").clone())
}

/// Connect a fresh session from cached OAuth credentials (no re-login needed —
/// the cached credential is long-lived). `Session::new` calls
/// `Handle::current()`, so it must be built inside the runtime.
fn connect_session(runtime: &Runtime) -> Result<Session> {
    let cache = open_cache()?;
    let credentials = cache
        .credentials()
        .ok_or_else(|| eyre!("not logged in to Spotify — run `looper spotify login` first"))?;
    runtime
        .block_on(async move {
            let session = Session::new(SessionConfig::default(), Some(cache));
            session.connect(credentials, true).await.map(|()| session)
        })
        .wrap_err("failed to connect to Spotify (is your Premium login still valid?)")
}

fn open_cache() -> Result<Cache> {
    let dir = crate::plugin::cache_dir_path()?.join("spotify");
    let audio_dir = dir.join("audio");
    std::fs::create_dir_all(&audio_dir).wrap_err("failed to create Spotify cache directory")?;
    Cache::new(
        Some(dir.clone()),
        Some(dir),
        Some(audio_dir),
        Some(AUDIO_CACHE_BYTES),
    )
    .map_err(|e| eyre!("failed to open Spotify cache: {e}"))
}

fn build_runtime() -> Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .wrap_err("failed to create Spotify async runtime")
}

/// Accepts `spotify:track:<id>` URIs and `https://open.spotify.com/<type>/<id>`
/// share URLs (including `/intl-xx/` locale prefixes and `?si=` query strings).
fn parse_uri(url: &str) -> Result<SpotifyUri> {
    let url = url.trim();
    if url.starts_with("spotify:") {
        return SpotifyUri::from_uri(url).map_err(|e| eyre!("invalid Spotify URI: {e}"));
    }
    if let Some(rest) = url.split("open.spotify.com/").nth(1) {
        let path = rest.split(['?', '#']).next().unwrap_or("");
        let segments: Vec<&str> = path
            .split('/')
            .filter(|s| !s.is_empty() && !s.starts_with("intl-"))
            .collect();
        if segments.len() >= 2 {
            let uri = format!("spotify:{}:{}", segments[0], segments[1]);
            return SpotifyUri::from_uri(&uri)
                .map_err(|e| eyre!("unsupported Spotify URL ({}): {e}", segments[0]));
        }
    }
    Err(eyre!("not a recognized Spotify URL: {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_spotify_links() {
        assert!(is_spotify_url("spotify:track:4uLU6hMCjMI75M1A2tKUQC"));
        assert!(is_spotify_url(
            "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=abc"
        ));
        assert!(!is_spotify_url("https://soundcloud.com/artist/track"));
        assert!(!is_spotify_url("/Users/me/music/focus.mp3"));
    }

    #[test]
    fn parses_track_url_to_uri() {
        let uri = parse_uri("https://open.spotify.com/track/4uLU6hMCjMI75M1A2tKUQC?si=x").unwrap();
        assert_eq!(uri.to_uri().unwrap(), "spotify:track:4uLU6hMCjMI75M1A2tKUQC");
    }

    #[test]
    fn parses_intl_prefixed_url() {
        let uri =
            parse_uri("https://open.spotify.com/intl-de/track/4uLU6hMCjMI75M1A2tKUQC").unwrap();
        assert_eq!(uri.to_uri().unwrap(), "spotify:track:4uLU6hMCjMI75M1A2tKUQC");
    }

    #[test]
    fn parses_playlist_and_album_urls() {
        assert!(matches!(
            parse_uri("https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M").unwrap(),
            SpotifyUri::Playlist { .. }
        ));
        assert!(matches!(
            parse_uri("https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy?si=x").unwrap(),
            SpotifyUri::Album { .. }
        ));
    }
}
