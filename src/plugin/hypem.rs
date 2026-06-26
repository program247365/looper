use color_eyre::eyre::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{ytdlp, ServicePlugin, TrackInfo};

const BROWSER_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

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

/// Fetch a HypeM track's artwork and cache it, returning the cached path.
///
/// yt-dlp's HypeM extractor returns no `thumbnail` field, so the normal
/// `ytdlp::fetch_thumbnail` path produces nothing. HypeM track pages still
/// carry a stable Open Graph `og:image` (used for social-share previews) whose
/// URL we can't construct from the id alone (the storage shard isn't derivable),
/// so we read the page and extract it. Returns `None` for non-HypeM URLs or any
/// failure — callers treat that as "no art" and fall back to the bundled cover.
pub fn fetch_thumbnail(url: &str, cache_dir: &Path) -> Option<PathBuf> {
    let id = track_id(url)?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(BROWSER_UA)
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;
    let html = client.get(url).send().ok()?.error_for_status().ok()?.text().ok()?;
    let image_url = extract_og_image(&html)?;
    let bytes = client
        .get(&image_url)
        .send()
        .ok()?
        .error_for_status()
        .ok()?
        .bytes()
        .ok()?;
    let path = cache_dir.join(format!("hypem_{id}.jpg"));
    std::fs::write(&path, &bytes).ok()?;
    Some(path)
}

/// Extracts the HypeM track id from a `hypem.com/track/<id>` URL. Ids are five
/// lowercase-alphanumeric chars; returns `None` for non-HypeM/non-track URLs.
fn track_id(url: &str) -> Option<String> {
    if !url.to_ascii_lowercase().contains("hypem.com") {
        return None;
    }
    let after = url.split("/track/").nth(1)?;
    let id: String = after
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    (id.len() == 5).then_some(id)
}

/// Pulls the `og:image` URL out of a page's `<meta>` tags, tolerating either
/// attribute order (`property` before or after `content`).
fn extract_og_image(html: &str) -> Option<String> {
    html.split("<meta")
        .find(|tag| tag.contains("og:image"))
        .and_then(|tag| {
            let start = tag.find("content=\"")? + "content=\"".len();
            let rest = &tag[start..];
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_track_id() {
        assert_eq!(
            track_id("https://hypem.com/track/39jtp/Artist+-+Song").as_deref(),
            Some("39jtp")
        );
        assert_eq!(track_id("https://hypem.com/track/39jtp").as_deref(), Some("39jtp"));
        assert_eq!(track_id("https://soundcloud.com/foo/bar"), None);
        assert_eq!(track_id("https://hypem.com/popular"), None);
    }

    #[test]
    fn extracts_og_image_property_first() {
        let html = r#"<meta property="og:image" content="https://static.hypem.com/x_500.jpg">"#;
        assert_eq!(
            extract_og_image(html).as_deref(),
            Some("https://static.hypem.com/x_500.jpg")
        );
    }

    #[test]
    fn extracts_og_image_content_first() {
        let html = r#"<meta content="https://static.hypem.com/y_500.jpg" property="og:image" />"#;
        assert_eq!(
            extract_og_image(html).as_deref(),
            Some("https://static.hypem.com/y_500.jpg")
        );
    }

    #[test]
    fn no_og_image_returns_none() {
        assert_eq!(extract_og_image("<html><head></head></html>"), None);
    }
}
