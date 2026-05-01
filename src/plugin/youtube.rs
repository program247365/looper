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
        ytdlp::resolve_url(&normalized, cache_dir, "YouTube")
    }
}

fn normalize_youtube_url(url: &str) -> String {
    if !url.contains("youtube.com/watch") || !url.contains("v=") || !url.contains("list=") {
        return url.to_string();
    }

    let base = match url.split_once('#') {
        Some((before, _)) => before,
        None => url,
    };
    let query = match base.split_once('?') {
        Some((_, q)) => q,
        None => return url.to_string(),
    };

    let list_param = query
        .split('&')
        .find(|part| part.starts_with("list="));

    match list_param {
        Some(list) => format!("https://www.youtube.com/playlist?{list}"),
        None => url.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_youtube_url;

    #[test]
    fn watch_with_list_becomes_playlist() {
        let got = normalize_youtube_url("https://www.youtube.com/watch?v=abc&list=PLxyz");
        assert_eq!(got, "https://www.youtube.com/playlist?list=PLxyz");
    }

    #[test]
    fn watch_without_list_is_unchanged() {
        let url = "https://www.youtube.com/watch?v=abc";
        assert_eq!(normalize_youtube_url(url), url);
    }

    #[test]
    fn playlist_url_is_unchanged() {
        let url = "https://www.youtube.com/playlist?list=PLxyz";
        assert_eq!(normalize_youtube_url(url), url);
    }

    #[test]
    fn extra_params_are_dropped_in_favor_of_list() {
        let got = normalize_youtube_url(
            "https://www.youtube.com/watch?v=abc&list=PLxyz&index=3&t=42s",
        );
        assert_eq!(got, "https://www.youtube.com/playlist?list=PLxyz");
    }
}
