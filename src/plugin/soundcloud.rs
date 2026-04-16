use color_eyre::eyre::Result;
use std::path::Path;

use super::{ytdlp, ServicePlugin, TrackInfo};

pub struct SoundCloud;

impl ServicePlugin for SoundCloud {
    fn name(&self) -> &str {
        "soundcloud"
    }

    fn matches_url(&self, url: &str) -> bool {
        url.to_ascii_lowercase().contains("soundcloud.com")
    }

    fn resolve(&self, url: &str, cache_dir: &Path) -> Result<Vec<TrackInfo>> {
        let tracks = ytdlp::resolve_streaming_tracks(url)?;
        if tracks.is_empty() {
            return ytdlp::resolve_url(url, cache_dir, "SoundCloud");
        }
        Ok(tracks)
    }
}
