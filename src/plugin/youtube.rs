use color_eyre::eyre::Result;
use std::path::Path;

use super::{ytdlp, ServicePlugin, TrackInfo};

pub struct YouTube;

impl ServicePlugin for YouTube {
    fn name(&self) -> &str {
        "youtube"
    }

    fn matches_url(&self, url: &str) -> bool {
        let url = url.to_ascii_lowercase();
        url.contains("youtube.com") || url.contains("youtu.be") || url.contains("music.youtube.com")
    }

    fn resolve(&self, url: &str, cache_dir: &Path) -> Result<Vec<TrackInfo>> {
        let normalized = normalize_youtube_url(url);
        if normalized != url {
            eprintln!(
                "YouTube URL includes both a video and a playlist. looper will play the single video and ignore the playlist context. If you intended playlist playback, use the playlist URL directly. Private playlists cannot be accessed by yt-dlp."
            );
        }
        ytdlp::resolve_url(&normalized, cache_dir, "YouTube")
    }
}

fn normalize_youtube_url(url: &str) -> String {
    if !url.contains("youtube.com/watch") || !url.contains("v=") || !url.contains("list=") {
        return url.to_string();
    }

    let (base, fragment) = match url.split_once('#') {
        Some((before, after)) => (before, Some(after)),
        None => (url, None),
    };
    let (path, query) = match base.split_once('?') {
        Some(parts) => parts,
        None => return url.to_string(),
    };

    let kept: Vec<&str> = query
        .split('&')
        .filter(|part| part.starts_with("v="))
        .collect();

    if kept.is_empty() {
        return url.to_string();
    }

    let mut normalized = format!("{path}?{}", kept.join("&"));
    if let Some(fragment) = fragment {
        normalized.push('#');
        normalized.push_str(fragment);
    }
    normalized
}
