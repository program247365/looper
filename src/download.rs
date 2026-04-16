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
