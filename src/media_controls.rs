use color_eyre::eyre::{eyre, Result};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
use std::path::Path;
use std::sync::{
    mpsc::{Receiver, Sender},
    Arc, Mutex,
};
use std::time::Duration;

use crate::play_loop::KeyCommand;
use crate::plugin::TrackInfo;

pub struct MediaSession {
    controls: Arc<Mutex<MediaControls>>,
}

impl MediaSession {
    pub fn start() -> Result<(Self, Receiver<KeyCommand>)> {
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<KeyCommand>();
        let session = Self::start_with_sender(cmd_tx)?;
        Ok((session, cmd_rx))
    }

    fn start_with_sender(cmd_tx: Sender<KeyCommand>) -> Result<Self> {
        #[cfg(target_os = "windows")]
        let hwnd = None;
        #[cfg(not(target_os = "windows"))]
        let hwnd = None;

        let config = PlatformConfig {
            display_name: "looper",
            dbus_name: "looper",
            hwnd,
        };
        let mut controls = MediaControls::new(config)
            .map_err(|e| eyre!("failed to create media controls: {e:?}"))?;

        controls
            .attach(move |event| {
                let cmd = match event {
                    MediaControlEvent::Play
                    | MediaControlEvent::Pause
                    | MediaControlEvent::Toggle => KeyCommand::TogglePause,
                    MediaControlEvent::Next => KeyCommand::NextTrack,
                    MediaControlEvent::Previous => KeyCommand::PreviousTrack,
                    MediaControlEvent::Stop | MediaControlEvent::Quit => KeyCommand::Quit,
                    _ => return,
                };
                let _ = cmd_tx.send(cmd);
            })
            .map_err(|e| eyre!("failed to attach media controls: {e:?}"))?;

        Ok(Self {
            controls: Arc::new(Mutex::new(controls)),
        })
    }

    pub fn handle(&self) -> MediaSessionHandle {
        MediaSessionHandle {
            controls: Arc::clone(&self.controls),
        }
    }
}

#[derive(Clone)]
pub struct MediaSessionHandle {
    controls: Arc<Mutex<MediaControls>>,
}

impl MediaSessionHandle {
    pub fn set_metadata(&self, track: &TrackInfo) {
        // souvlaki loads the cover via NSURL (macOS) / MPRIS URI (Linux), so the
        // downloaded album art / thumbnail has to be handed over as a `file://`
        // URL. `cover_url` borrows the string, so it must outlive the call.
        let cover_url = track.thumbnail_path.as_deref().map(file_url);
        let mut controls = self.controls.lock().unwrap();
        let _ = controls.set_metadata(MediaMetadata {
            title: Some(&track.title),
            // Real artist when the source gave us one; otherwise the service
            // name so the widget subtitle is never empty.
            artist: track.artist.as_deref().or(track.service.as_deref()),
            album: track.collection.as_deref(),
            cover_url: cover_url.as_deref(),
            duration: track.duration_secs.map(Duration::from_secs_f64),
            ..Default::default()
        });
    }

    pub fn set_playback(&self, paused: bool, progress: Duration) {
        let mut controls = self.controls.lock().unwrap();
        let position = MediaPosition(progress);
        let state = if paused {
            MediaPlayback::Paused {
                progress: Some(position),
            }
        } else {
            MediaPlayback::Playing {
                progress: Some(position),
            }
        };
        let _ = controls.set_playback(state);
    }
}

/// Build a `file://` URL for a local path. souvlaki passes `cover_url` straight
/// to `NSURL URLWithString:` (macOS) / a MPRIS URI (Linux), both of which need a
/// percent-encoded URL — so a raw path with spaces would silently fail to load.
/// Keeps RFC 3986 unreserved characters and `/`, percent-encodes everything else
/// per UTF-8 byte.
fn file_url(path: &Path) -> String {
    let mut url = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        match byte {
            b'/' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                url.push(byte as char)
            }
            _ => url.push_str(&format!("%{byte:02X}")),
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use super::file_url;
    use std::path::Path;

    #[test]
    fn encodes_spaces_and_keeps_slashes() {
        assert_eq!(
            file_url(Path::new("/Users/First Last/art/a b.jpg")),
            "file:///Users/First%20Last/art/a%20b.jpg"
        );
    }

    #[test]
    fn leaves_plain_paths_untouched() {
        assert_eq!(
            file_url(Path::new(
                "/Users/kevin/Library/Caches/sh.kbr.looper/spotify/art/ab12.jpg"
            )),
            "file:///Users/kevin/Library/Caches/sh.kbr.looper/spotify/art/ab12.jpg"
        );
    }
}
