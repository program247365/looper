use color_eyre::eyre::{eyre, Result};
use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
};
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
        let mut controls = self.controls.lock().unwrap();
        let _ = controls.set_metadata(MediaMetadata {
            title: Some(&track.title),
            artist: track.service.as_deref(),
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
