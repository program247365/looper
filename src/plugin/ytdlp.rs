use color_eyre::eyre::{bail, eyre, Result, WrapErr};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use crate::download::{DownloadEvent, DownloadProgress};
use crate::playback_input::PendingDownload;
use crate::playback_input::{PlaybackInput, ProcessFormat};

use super::TrackInfo;

#[derive(Debug)]
pub struct MetadataEntry {
    pub id: String,
    pub title: String,
    pub duration_secs: Option<f64>,
    pub webpage_url: String,
}

pub fn check_installed() -> Result<()> {
    let output = Command::new("yt-dlp")
        .arg("--version")
        .output()
        .wrap_err("failed to execute yt-dlp; install yt-dlp to play online URLs")?;

    if output.status.success() {
        return Ok(());
    }

    bail!("yt-dlp is required to play online URLs; install yt-dlp and ffmpeg, then try again");
}

pub fn resolve_url(url: &str, cache_dir: &Path, service: &str) -> Result<Vec<TrackInfo>> {
    let tracks = extract_metadata(url)?;
    if tracks.is_empty() {
        bail!("yt-dlp returned no playable tracks for {url}");
    }

    tracks
        .into_iter()
        .map(|track| {
            let local_path = cache_path(cache_dir, &track.id);
            let pending_download = if local_path.exists() {
                None
            } else {
                Some(PendingDownload {
                    source_url: track.webpage_url.clone(),
                })
            };
            let thumbnail_path = thumbnail_for(cache_dir, &track.id);
            Ok(TrackInfo {
                title: track.title,
                duration_secs: track.duration_secs,
                playback: PlaybackInput::file(local_path),
                source_url: Some(track.webpage_url),
                pending_download,
                service: Some(service.to_string()),
                thumbnail_path,
            })
        })
        .collect()
}

pub fn resolve_streaming_tracks(url: &str) -> Result<Vec<TrackInfo>> {
    let tracks = extract_playlist_metadata(url)?;
    if tracks.is_empty() {
        bail!("yt-dlp returned no playable tracks for {url}");
    }

    tracks
        .into_iter()
        .map(|entry| {
            let service = service_label(&entry.webpage_url).to_string();
            let playback = resolve_stream_url(&entry.webpage_url)?;
            Ok(TrackInfo {
                title: entry.title,
                duration_secs: entry.duration_secs,
                playback,
                source_url: Some(entry.webpage_url),
                pending_download: None,
                service: Some(service),
                thumbnail_path: None,
            })
        })
        .collect()
}

pub fn resolve_stream_url(url: &str) -> Result<PlaybackInput> {
    verify_stream_access(url)?;
    Ok(PlaybackInput::process_stdout(
        "yt-dlp",
        vec![
            url.to_string(),
            "--quiet".to_string(),
            "--no-part".to_string(),
            "--no-continue".to_string(),
            "--no-playlist".to_string(),
            "--no-update".to_string(),
            "-f".to_string(),
            "bestaudio/best".to_string(),
            "-o".to_string(),
            "-".to_string(),
        ],
        ProcessFormat::Wav,
    ))
}

fn verify_stream_access(url: &str) -> Result<()> {
    let output = Command::new("yt-dlp")
        .arg("-f")
        .arg("bestaudio/best")
        .arg("--no-playlist")
        .arg("--no-update")
        .arg("--get-url")
        .arg(url)
        .output()
        .wrap_err("failed to execute yt-dlp stream accessibility check")?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    bail!(
        "{}",
        explain_ytdlp_failure("access stream", url, stderr.trim())
    );
}

fn extract_metadata(url: &str) -> Result<Vec<MetadataEntry>> {
    extract_entries(url)
}

pub fn extract_playlist_metadata(url: &str) -> Result<Vec<MetadataEntry>> {
    extract_entries(url)
}

pub fn download_track(url: &str, cache_dir: &Path) -> Result<PathBuf> {
    download_track_with_progress(url, cache_dir, None)
}

/// Look for a thumbnail file written by `yt-dlp --write-thumbnail` for `id`.
/// Probes the four extensions yt-dlp commonly emits.
pub fn thumbnail_for(cache_dir: &Path, id: &str) -> Option<PathBuf> {
    for ext in &["jpg", "jpeg", "png", "webp"] {
        let candidate = cache_dir.join(format!("{id}.{ext}"));
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

/// Backfill: fetch only the thumbnail for `url` into `cache_dir`. Used when
/// the audio is already cached (so the audio download path doesn't run) but
/// no thumbnail file exists yet — typical for tracks downloaded before the
/// --write-thumbnail flag was added.
///
/// Non-fatal: errors are returned but the caller is expected to swallow them.
pub fn download_thumbnail_only(url: &str, cache_dir: &Path) -> Result<()> {
    let output_template = cache_dir.join("%(id)s.%(ext)s");
    let status = Command::new("yt-dlp")
        .arg("--skip-download")
        .arg("--write-thumbnail")
        .arg("--convert-thumbnails")
        .arg("jpg")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("-o")
        .arg(&output_template)
        .arg(url)
        .status()
        .wrap_err("failed to execute yt-dlp thumbnail backfill")?;
    if !status.success() {
        bail!("yt-dlp thumbnail backfill exited with non-success status");
    }
    Ok(())
}

pub fn download_track_with_progress(
    url: &str,
    cache_dir: &Path,
    progress_sender: Option<Sender<DownloadEvent>>,
) -> Result<PathBuf> {
    let output_template = cache_dir.join("%(id)s.%(ext)s");
    let mut command = Command::new("yt-dlp");
    command
        .arg("-x")
        .arg("--audio-format")
        .arg("mp3")
        .arg("--no-playlist")
        .arg("--no-warnings")
        .arg("--newline")
        .arg("--write-thumbnail")
        .arg("--convert-thumbnails")
        .arg("jpg")
        .arg("--progress-template")
        .arg("download:LOOPER_PROGRESS\t%(progress.downloaded_bytes)s\t%(progress.total_bytes)s\t%(progress.total_bytes_estimate)s\t%(progress.speed)s\t%(progress.eta)s")
        .arg("-o")
        .arg(&output_template)
        .arg("--print")
        .arg("after_move:filepath")
        .arg(url)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = command
        .spawn()
        .wrap_err("failed to execute yt-dlp download")?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| eyre!("yt-dlp stdout was not captured"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| eyre!("yt-dlp stderr was not captured"))?;

    let stdout_handle = std::thread::spawn(move || collect_stdout(stdout));
    let stderr_lines = read_progress(stderr, progress_sender.as_ref());

    let status = child
        .wait()
        .wrap_err("failed to wait for yt-dlp download")?;
    let stdout = stdout_handle
        .join()
        .map_err(|_| eyre!("failed to join yt-dlp stdout reader"))??;
    let stderr = stderr_lines?;

    if !status.success() {
        bail!(
            "{}",
            explain_ytdlp_failure("download audio", url, stderr.trim())
        );
    }

    let path = stdout
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| eyre!("yt-dlp did not report the downloaded file path"))?;

    if let Some(sender) = progress_sender {
        let _ = sender.send(DownloadEvent::Ready(PathBuf::from(path.trim())));
    }

    Ok(PathBuf::from(path.trim()))
}

fn extract_entries(url: &str) -> Result<Vec<MetadataEntry>> {
    let stdout = run_ytdlp(&[
        "--dump-json",
        "--flat-playlist",
        "--no-warnings",
        "--skip-download",
        url,
    ])
    .wrap_err("failed to execute yt-dlp metadata extraction")?;
    let mut entries = Vec::new();

    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let value: Value =
            serde_json::from_str(line).wrap_err("failed to parse yt-dlp metadata")?;
        entries.push(parse_entry(value, url)?);
    }

    if entries.is_empty()
        || entries
            .iter()
            .any(|entry| !is_remote_url(&entry.webpage_url))
    {
        return extract_entries_fallback(url);
    }

    Ok(entries)
}

fn extract_entries_fallback(url: &str) -> Result<Vec<MetadataEntry>> {
    let stdout = run_ytdlp(&[
        "--dump-single-json",
        "--no-warnings",
        "--skip-download",
        url,
    ])
    .wrap_err("failed to execute yt-dlp metadata fallback")?;
    let value: Value =
        serde_json::from_str(stdout.trim()).wrap_err("failed to parse yt-dlp metadata")?;

    if let Some(items) = value.get("entries").and_then(|v| v.as_array()) {
        items
            .iter()
            .cloned()
            .map(|item| parse_entry(item, url))
            .collect()
    } else {
        Ok(vec![parse_entry(value, url)?])
    }
}

fn parse_entry(value: Value, fallback_url: &str) -> Result<MetadataEntry> {
    let title = value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or("Unknown track")
        .to_string();
    let id = value
        .get("id")
        .and_then(Value::as_str)
        .map(sanitize_id)
        .unwrap_or_else(|| sanitize_id(&title));

    let duration_secs = value.get("duration").and_then(Value::as_f64);
    let webpage_url = youtube_watch_url(&value)
        .or_else(|| {
            first_str(&value, &["webpage_url", "original_url", "url"])
                .filter(|candidate| is_remote_url(candidate))
                .map(str::to_string)
        })
        .unwrap_or_else(|| fallback_url.to_string());

    Ok(MetadataEntry {
        id,
        title,
        duration_secs,
        webpage_url,
    })
}

fn first_str<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
}

fn youtube_watch_url(value: &Value) -> Option<String> {
    let extractor = first_str(value, &["ie_key", "extractor_key"])?;
    let id = value.get("id").and_then(Value::as_str)?;

    if extractor.eq_ignore_ascii_case("youtube") {
        Some(format!("https://www.youtube.com/watch?v={id}"))
    } else {
        None
    }
}

fn is_remote_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn run_ytdlp(args: &[&str]) -> Result<String> {
    let output = Command::new("yt-dlp").args(args).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let url = args.last().copied().unwrap_or("");
        bail!(
            "{}",
            explain_ytdlp_failure("inspect URL", url, stderr.trim())
        );
    }

    String::from_utf8(output.stdout).wrap_err("yt-dlp emitted non-utf8 metadata")
}

fn cache_path(cache_dir: &Path, id: &str) -> PathBuf {
    cache_dir.join(format!("{id}.mp3"))
}

fn service_label(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    if lower.contains("youtube.com")
        || lower.contains("youtu.be")
        || lower.contains("music.youtube.com")
    {
        "YouTube"
    } else if lower.contains("soundcloud.com") {
        "SoundCloud"
    } else if lower.contains("hypem.com") {
        "HypeM"
    } else {
        "Online"
    }
}

fn sanitize_id(value: &str) -> String {
    let sanitized: String = value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "track".to_string()
    } else {
        sanitized
    }
}

fn collect_stdout(stdout: impl std::io::Read) -> Result<String> {
    let mut buf = String::new();
    let mut reader = BufReader::new(stdout);
    std::io::Read::read_to_string(&mut reader, &mut buf)
        .wrap_err("yt-dlp emitted non-utf8 output")?;
    Ok(buf)
}

fn read_progress(
    stderr: impl std::io::Read,
    progress_sender: Option<&Sender<DownloadEvent>>,
) -> Result<String> {
    let mut stderr_buf = String::new();
    let reader = BufReader::new(stderr);

    for line in reader.lines() {
        let line = line.wrap_err("failed to read yt-dlp progress output")?;
        stderr_buf.push_str(&line);
        stderr_buf.push('\n');

        if let Some(progress) = parse_progress_line(&line) {
            if let Some(sender) = progress_sender {
                let _ = sender.send(DownloadEvent::Progress(progress));
            }
        } else if line.contains("Destination:") || line.contains("Post-process") {
            if let Some(sender) = progress_sender {
                let _ = sender.send(DownloadEvent::Finalizing);
            }
        }
    }

    Ok(stderr_buf)
}

fn parse_progress_line(line: &str) -> Option<DownloadProgress> {
    let payload = line.strip_prefix("LOOPER_PROGRESS\t")?;
    let mut parts = payload.split('\t');
    let downloaded_bytes = parse_u64(parts.next()?);
    let total_bytes = parse_u64(parts.next()?);
    let total_bytes_estimate = parse_u64(parts.next()?);
    let speed_bytes_per_sec = parse_u64(parts.next()?);
    let eta_seconds = parse_u64(parts.next()?);

    Some(DownloadProgress {
        downloaded_bytes,
        total_bytes: total_bytes.or(total_bytes_estimate),
        speed_bytes_per_sec,
        eta_seconds,
    })
}

fn parse_u64(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("na")
        || trimmed.eq_ignore_ascii_case("none")
    {
        return None;
    }

    trimmed.parse::<f64>().ok().map(|v| v.max(0.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::parse_progress_line;

    #[test]
    fn parses_progress_line_with_total_bytes() {
        let progress =
            parse_progress_line("LOOPER_PROGRESS\t1048576\t4194304\tNA\t524288\t6").unwrap();
        assert_eq!(progress.downloaded_bytes, Some(1_048_576));
        assert_eq!(progress.total_bytes, Some(4_194_304));
        assert_eq!(progress.speed_bytes_per_sec, Some(524_288));
        assert_eq!(progress.eta_seconds, Some(6));
    }

    #[test]
    fn falls_back_to_estimated_total_bytes() {
        let progress = parse_progress_line("LOOPER_PROGRESS\t2048\tNA\t4096\tNA\tNA").unwrap();
        assert_eq!(progress.downloaded_bytes, Some(2_048));
        assert_eq!(progress.total_bytes, Some(4_096));
        assert_eq!(progress.speed_bytes_per_sec, None);
        assert_eq!(progress.eta_seconds, None);
    }
}

fn explain_ytdlp_failure(action: &str, url: &str, stderr: &str) -> String {
    if is_youtube_url(url) && looks_like_youtube_access_issue(stderr) {
        return format!(
            "yt-dlp could not {action}. YouTube denied access to this content. This can mean the video or playlist is private, members-only, age-restricted, or that the installed yt-dlp is too old for YouTube's current streaming behavior. Update yt-dlp first; if that still fails, try cookies/authenticated access. If your URL includes both `v=` and `list=`, try the plain video URL or a public playlist URL. Original error: {stderr}"
        );
    }

    if is_hypem_url(url) && looks_like_not_found(stderr) {
        return format!(
            "yt-dlp could not {action}. The HypeM URL appears invalid, removed, or unavailable. Original error: {stderr}"
        );
    }

    format!("yt-dlp failed to {action}: {stderr}")
}

fn is_youtube_url(url: &str) -> bool {
    url.contains("youtube.com") || url.contains("youtu.be") || url.contains("music.youtube.com")
}

fn is_hypem_url(url: &str) -> bool {
    url.contains("hypem.com")
}

fn looks_like_youtube_access_issue(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("403")
        || stderr.contains("forbidden")
        || stderr.contains("private")
        || stderr.contains("members-only")
        || stderr.contains("sign in")
        || stderr.contains("unavailable")
}

fn looks_like_not_found(stderr: &str) -> bool {
    let stderr = stderr.to_ascii_lowercase();
    stderr.contains("404") || stderr.contains("not found")
}
