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
}

#[derive(Clone, Debug)]
pub enum ProcessFormat {
    Wav,
}

#[derive(Clone, Debug)]
pub struct PendingDownload {
    pub service: String,
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
}
