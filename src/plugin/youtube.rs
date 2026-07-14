use color_eyre::eyre::{bail, Result};
use std::path::Path;

use super::{ytdlp, ServicePlugin, TrackInfo};
use ytdlp::LiveStatus;

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
        let entries = ytdlp::extract_metadata(&normalized)?;
        if entries.is_empty() {
            bail!("yt-dlp returned no playable tracks for {normalized}");
        }

        entries
            .into_iter()
            .map(|entry| match entry.live_status {
                LiveStatus::IsLive => {
                    let id = entry.id.clone();
                    let mut track = ytdlp::streaming_track_from_entry(entry, "YouTube")?;
                    if let Some(source_url) = track.source_url.clone() {
                        let _ = ytdlp::download_thumbnail_only(&source_url, cache_dir);
                        track.thumbnail_path = ytdlp::thumbnail_for(cache_dir, &id);
                    }
                    Ok(track)
                }
                LiveStatus::IsUpcoming => bail!(
                    "`{}` is a scheduled livestream that hasn't started yet; try again once it goes live",
                    entry.title
                ),
                LiveStatus::WasLive | LiveStatus::NotLive => {
                    Ok(ytdlp::cached_track_from_entry(entry, cache_dir, "YouTube"))
                }
            })
            .collect()
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

    let list_param = query.split('&').find(|part| part.starts_with("list="));

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
        let got =
            normalize_youtube_url("https://www.youtube.com/watch?v=abc&list=PLxyz&index=3&t=42s");
        assert_eq!(got, "https://www.youtube.com/playlist?list=PLxyz");
    }
}
