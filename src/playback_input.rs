use std::path::PathBuf;

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum PlaybackInput {
    File(PathBuf),
    HttpStream {
        url: String,
        headers: Vec<(String, String)>,
    },
    ProcessStdout {
        program: String,
        args: Vec<String>,
        format_hint: ProcessFormat,
    },
    /// A Spotify track, decoded in-process by librespot. The string is a
    /// canonical Spotify track URI (`spotify:track:<base62>`). Unlike the
    /// other variants there is no file or byte stream — `AudioPlayer` hands
    /// this to the librespot bridge, which pushes decoded PCM into a rodio
    /// `Source`.
    Spotify {
        track_uri: String,
    },
}

#[derive(Clone, Debug)]
pub enum ProcessFormat {
    Wav,
}

#[derive(Clone, Debug)]
pub struct PendingDownload {
    pub source_url: String,
}

impl PlaybackInput {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self::File(path.into())
    }

    pub fn process_stdout(
        program: impl Into<String>,
        args: Vec<String>,
        format_hint: ProcessFormat,
    ) -> Self {
        Self::ProcessStdout {
            program: program.into(),
            args,
            format_hint,
        }
    }

    pub fn spotify(track_uri: impl Into<String>) -> Self {
        Self::Spotify {
            track_uri: track_uri.into(),
        }
    }
}
