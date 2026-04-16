use color_eyre::eyre::Result;
use std::path::Path;

use super::{ytdlp, ServicePlugin, TrackInfo};

pub struct HypeM;

impl ServicePlugin for HypeM {
    fn name(&self) -> &str {
        "hypem"
    }

    fn matches_url(&self, url: &str) -> bool {
        url.to_ascii_lowercase().contains("hypem.com")
    }

    fn resolve(&self, url: &str, cache_dir: &Path) -> Result<Vec<TrackInfo>> {
        let tracks = ytdlp::resolve_streaming_tracks(url)?;
        if tracks.is_empty() {
            return ytdlp::resolve_url(url, cache_dir, "HypeM");
        }
        Ok(tracks)
    }
}
