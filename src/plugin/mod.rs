use color_eyre::eyre::{eyre, Result, WrapErr};
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};

use crate::playback_input::{PendingDownload, PlaybackInput};

pub mod hypem;
mod soundcloud;
mod youtube;
pub mod ytdlp;

use hypem::HypeM;
use soundcloud::SoundCloud;
use youtube::YouTube;

#[derive(Clone, Debug)]
pub struct TrackInfo {
    pub title: String,
    pub duration_secs: Option<f64>,
    pub playback: PlaybackInput,
    pub source_url: Option<String>,
    pub pending_download: Option<PendingDownload>,
    pub service: Option<String>,
    pub thumbnail_path: Option<PathBuf>,
    pub is_live: bool,
    /// Name of the playlist or album this track was resolved from, if any.
    /// Shown in the playback header so you know what collection is playing.
    pub collection: Option<String>,
    /// Performing artist(s), when the source provides them. Surfaced in the OS
    /// Now Playing widget; falls back to the service name when absent.
    pub artist: Option<String>,
}

pub trait ServicePlugin {
    fn name(&self) -> &str;
    fn matches_url(&self, url: &str) -> bool;
    fn resolve(&self, url: &str, cache_dir: &Path) -> Result<Vec<TrackInfo>>;
}

pub fn resolve_url(url: &str) -> Result<Option<Vec<TrackInfo>>> {
    // Spotify is handled by librespot, not yt-dlp, and uses `spotify:` URIs that
    // aren't http(s). Intercept before the remote-URL and yt-dlp checks below.
    if crate::spotify::is_spotify_url(url) {
        return crate::spotify::resolve(url).map(Some);
    }

    if !is_remote_url(url) {
        return Ok(None);
    }

    ytdlp::check_installed()?;
    let cache_dir = cache_dir()?;

    for plugin in registry() {
        if plugin.matches_url(url) {
            return plugin
                .resolve(url, &cache_dir)
                .wrap_err_with(|| format!("{} plugin failed to resolve URL", plugin.name()))
                .map(Some);
        }
    }

    ytdlp::resolve_url(url, &cache_dir, "Online").map(Some)
}

pub fn cache_dir_path() -> Result<PathBuf> {
    cache_dir()
}

fn registry() -> Vec<Box<dyn ServicePlugin>> {
    vec![Box::new(YouTube), Box::new(SoundCloud), Box::new(HypeM)]
}

fn cache_dir() -> Result<PathBuf> {
    let dirs = ProjectDirs::from("sh", "kbr", "looper")
        .ok_or_else(|| eyre!("failed to determine cache directory for looper"))?;
    let dir = dirs.cache_dir().to_path_buf();
    fs::create_dir_all(&dir).wrap_err("failed to create looper cache directory")?;
    Ok(dir)
}

fn is_remote_url(url: &str) -> bool {
    matches!(url, s if s.starts_with("http://") || s.starts_with("https://"))
}
