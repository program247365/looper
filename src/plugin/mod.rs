use color_eyre::eyre::{eyre, Result, WrapErr};
use directories::ProjectDirs;
use std::fs;
use std::path::{Path, PathBuf};

use crate::playback_input::{PendingDownload, PlaybackInput};

mod hypem;
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
}

pub trait ServicePlugin {
    fn name(&self) -> &str;
    fn matches_url(&self, url: &str) -> bool;
    fn resolve(&self, url: &str, cache_dir: &Path) -> Result<Vec<TrackInfo>>;
}

pub fn resolve_url(url: &str) -> Result<Option<Vec<TrackInfo>>> {
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
