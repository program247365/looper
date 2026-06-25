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

mod sink;

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use color_eyre::eyre::{eyre, Result, WrapErr};

use librespot_core::authentication::Credentials;
use librespot_core::cache::Cache;
use librespot_core::config::SessionConfig;
use librespot_core::session::Session;
use librespot_core::spotify_uri::SpotifyUri;
use librespot_metadata::{Metadata, Track};
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

/// Resolve a Spotify URL/URI to playable tracks. Thin slice: single tracks only.
pub fn resolve(url: &str) -> Result<Vec<TrackInfo>> {
    let uri = parse_uri(url)?;
    match &uri {
        SpotifyUri::Track { .. } => {
            let ctx = ctx()?;
            let track = ctx
                .runtime
                .block_on(Track::get(&ctx.session, &uri))
                .map_err(|e| eyre!("failed to fetch Spotify track metadata: {e}"))?;
            let track_uri = uri
                .to_uri()
                .map_err(|e| eyre!("failed to build Spotify track URI: {e}"))?;
            Ok(vec![TrackInfo {
                title: track.name,
                duration_secs: Some((track.duration.max(0) as f64) / 1000.0),
                playback: PlaybackInput::spotify(track_uri),
                source_url: Some(url.to_string()),
                pending_download: None,
                service: Some("Spotify".to_string()),
                thumbnail_path: None,
                is_live: false,
            }])
        }
        SpotifyUri::Playlist { .. } | SpotifyUri::Album { .. } => Err(eyre!(
            "Spotify playlists and albums aren't supported yet — pass a track URL for now"
        )),
        _ => Err(eyre!("unsupported Spotify link — pass a track URL")),
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
    let uri =
        SpotifyUri::from_uri(track_uri).map_err(|e| eyre!("invalid Spotify track URI: {e}"))?;
    let (spotify_sink, source) = sink::bridge();

    let (player, duration, listener) = ctx.runtime.block_on(async move {
        let duration = match Track::get(&ctx.session, &uri).await {
            Ok(track) => Some(Duration::from_millis(track.duration.max(0) as u64)),
            Err(_) => None,
        };

        let player = Player::new(
            PlayerConfig::default(),
            ctx.session.clone(),
            Box::new(NoOpVolume),
            move || Box::new(spotify_sink),
        );

        let mut events = player.get_player_event_channel();
        player.load(uri.clone(), true, 0);

        // Re-load the same track each time it ends to loop it. The bridge
        // emits silence during the brief reload gap, so audio never stops.
        let listener = {
            let player = player.clone();
            let loop_uri = uri.clone();
            tokio::spawn(async move {
                while let Some(event) = events.recv().await {
                    if repeat && matches!(event, PlayerEvent::EndOfTrack { .. }) {
                        player.load(loop_uri.clone(), true, 0);
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

/// Shared, connected librespot session plus the runtime its tasks run on.
struct SpotifyCtx {
    runtime: Runtime,
    session: Session,
}

static CTX: OnceLock<SpotifyCtx> = OnceLock::new();

/// Lazily connect (once per process) and return the shared context. Calls
/// after the first reuse the same connection.
fn ctx() -> Result<&'static SpotifyCtx> {
    if let Some(ctx) = CTX.get() {
        return Ok(ctx);
    }
    let ctx = init_ctx()?;
    // A racing thread may have set it first; that's fine, drop ours.
    let _ = CTX.set(ctx);
    Ok(CTX.get().expect("context was just set"))
}

fn init_ctx() -> Result<SpotifyCtx> {
    let runtime = build_runtime()?;
    let cache = open_cache()?;
    let credentials = cache
        .credentials()
        .ok_or_else(|| eyre!("not logged in to Spotify — run `looper spotify login` first"))?;
    // `Session::new` calls `Handle::current()`, so build + connect it inside the
    // runtime context.
    let session = runtime
        .block_on(async move {
            let session = Session::new(SessionConfig::default(), Some(cache));
            session.connect(credentials, true).await.map(|()| session)
        })
        .wrap_err("failed to connect to Spotify (is your Premium login still valid?)")?;
    Ok(SpotifyCtx { runtime, session })
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
}
