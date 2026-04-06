use color_eyre::eyre::{Result, WrapErr};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::fs::File;
use std::io::BufReader;
use std::time::Duration;

pub struct AudioPlayer {
    _stream: OutputStream,
    _stream_handle: OutputStreamHandle,
    pub sink: Sink,
    pub duration: Option<Duration>,
}

impl AudioPlayer {
    pub fn new(path: &str) -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().wrap_err("failed to open audio output device")?;
        let sink = Sink::try_new(&handle).wrap_err("failed to create audio sink")?;

        // Probe duration from a fresh decoder (consumes the reader, so open twice)
        let duration = File::open(path)
            .ok()
            .and_then(|f| Decoder::new(BufReader::new(f)).ok())
            .and_then(|d| d.total_duration());

        // Playback decoder
        let file = File::open(path).wrap_err("failed to open audio file")?;
        let source = Decoder::new(BufReader::new(file))
            .wrap_err("failed to decode audio file")?
            .repeat_infinite();
        sink.append(source);

        Ok(Self {
            _stream: stream,
            _stream_handle: handle,
            sink,
            duration,
        })
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn resume(&self) {
        self.sink.play();
    }

}
