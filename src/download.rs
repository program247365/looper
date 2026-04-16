use std::path::PathBuf;

#[derive(Clone, Debug, Default)]
pub struct DownloadProgress {
    pub downloaded_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub speed_bytes_per_sec: Option<u64>,
    pub eta_seconds: Option<u64>,
}

impl DownloadProgress {
    pub fn fraction(&self) -> Option<f64> {
        let downloaded = self.downloaded_bytes?;
        let total = self.total_bytes?;
        if total == 0 {
            return None;
        }
        Some((downloaded as f64 / total as f64).clamp(0.0, 1.0))
    }
}

#[derive(Clone, Debug)]
pub enum LoadingPhase {
    Resolving,
    Downloading,
    Finalizing,
    Ready,
    Error(String),
}

#[derive(Clone, Debug)]
pub struct LoadingState {
    pub title: String,
    pub service: String,
    pub track_index: usize,
    pub total_tracks: usize,
    pub is_playlist: bool,
    pub progress: DownloadProgress,
    pub phase: LoadingPhase,
    pub frame_count: u64,
}

impl LoadingState {
    pub fn new(
        title: String,
        service: String,
        track_index: usize,
        total_tracks: usize,
        is_playlist: bool,
    ) -> Self {
        Self {
            title,
            service,
            track_index,
            total_tracks,
            is_playlist,
            progress: DownloadProgress::default(),
            phase: LoadingPhase::Resolving,
            frame_count: 0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct CacheStatus {
    pub progress: DownloadProgress,
    pub complete: bool,
}

#[derive(Clone, Debug)]
pub enum DownloadEvent {
    Progress(DownloadProgress),
    Finalizing,
    Ready(PathBuf),
    Error(String),
}

pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub fn format_speed(speed: Option<u64>) -> String {
    speed
        .map(|bytes| format!("{}/s", format_bytes(bytes)))
        .unwrap_or_else(|| "--".to_string())
}

pub fn format_eta(eta: Option<u64>) -> String {
    let Some(eta) = eta else {
        return "--:--".to_string();
    };

    let minutes = eta / 60;
    let seconds = eta % 60;
    if minutes >= 60 {
        let hours = minutes / 60;
        let minutes = minutes % 60;
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}
