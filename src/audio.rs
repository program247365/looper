use color_eyre::eyre::{Result, WrapErr};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stream_download::http::HttpStream;
use stream_download::process::{
    Command as ProcessCommand, CommandBuilder, FfmpegConvertAudioCommand, ProcessStreamParams,
};
use stream_download::storage::temp::TempStorageProvider;
use stream_download::{Settings, StreamDownload};
use tokio::runtime::Runtime;

use crate::playback_input::PlaybackInput;
use stream_download::http::reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use stream_download::http::reqwest::Client as ReqwestClient;

// Enough for 2048 mono FFT samples even with stereo input (2048 * 2 channels * 2x headroom)
const BUF_CAP: usize = 8192;

/// Wraps a Source and copies every sample into a shared ring buffer.
/// Uses try_lock so the audio thread never blocks waiting for the main thread.
pub struct SampleTap<S: Source<Item = f32>> {
    inner: S,
    buf: Arc<Mutex<VecDeque<f32>>>,
}

impl<S: Source<Item = f32>> Iterator for SampleTap<S> {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let s = self.inner.next()?;
        if let Ok(mut b) = self.buf.try_lock() {
            if b.len() >= BUF_CAP {
                b.pop_front();
            }
            b.push_back(s);
        }
        Some(s)
    }
}

impl<S: Source<Item = f32>> Source for SampleTap<S> {
    fn current_frame_len(&self) -> Option<usize> {
        self.inner.current_frame_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
}

pub struct AudioPlayer {
    _stream: OutputStream,
    _stream_handle: OutputStreamHandle,
    _download_runtime: Option<Runtime>,
    pub sink: Sink,
    pub duration: Option<Duration>,
    pub sample_buf: Arc<Mutex<VecDeque<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
}

trait MediaReader: Read + Seek + Send + Sync {}

impl<T: Read + Seek + Send + Sync> MediaReader for T {}

impl AudioPlayer {
    pub fn new(input: PlaybackInput, repeat: bool) -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().wrap_err("failed to open audio output device")?;
        let sink = Sink::try_new(&handle).wrap_err("failed to create audio sink")?;

        let (reader, duration, runtime) = open_input(&input)?;
        let source = decode_input(reader, &input)?.convert_samples::<f32>();

        let sample_rate = source.sample_rate();
        let channels = source.channels();

        let buf = Arc::new(Mutex::new(VecDeque::with_capacity(BUF_CAP)));
        if repeat {
            let tapped = SampleTap {
                inner: source.repeat_infinite(),
                buf: buf.clone(),
            };
            sink.append(tapped);
        } else {
            let tapped = SampleTap {
                inner: source,
                buf: buf.clone(),
            };
            sink.append(tapped);
        }

        Ok(Self {
            _stream: stream,
            _stream_handle: handle,
            _download_runtime: runtime,
            sink,
            duration,
            sample_buf: buf,
            sample_rate,
            channels,
        })
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn resume(&self) {
        self.sink.play();
    }
}

fn decode_input(
    reader: Box<dyn MediaReader>,
    _input: &PlaybackInput,
) -> Result<Decoder<BufReader<Box<dyn MediaReader>>>> {
    let reader = BufReader::new(reader);
    Decoder::new(reader).wrap_err("failed to decode audio")
}

fn open_input(
    input: &PlaybackInput,
) -> Result<(Box<dyn MediaReader>, Option<Duration>, Option<Runtime>)> {
    match input {
        PlaybackInput::File(path) => {
            let path_str = path.to_string_lossy();
            let duration = File::open(path)
                .ok()
                .and_then(|f| Decoder::new(BufReader::new(f)).ok())
                .and_then(|d| d.total_duration())
                .or_else(|| probe_duration_symphonia(&path_str));

            let file = File::open(path).wrap_err("failed to open audio file")?;
            Ok((Box::new(file), duration, None))
        }
        PlaybackInput::HttpStream { url, headers } => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .wrap_err("failed to create async runtime for HTTP audio streaming")?;
            let client = build_http_client(headers)?;
            let reader = runtime
                .block_on(async {
                    let stream = HttpStream::new(
                        client,
                        url.parse().wrap_err("failed to parse stream URL")?,
                    )
                    .await
                    .wrap_err("failed to create HTTP audio stream")?;

                    StreamDownload::from_stream(
                        stream,
                        TempStorageProvider::new(),
                        Settings::default(),
                    )
                    .await
                    .wrap_err("failed to open HTTP audio stream")
                })
                .wrap_err("failed to open HTTP audio stream")?;
            Ok((Box::new(reader), None, Some(runtime)))
        }
        PlaybackInput::ProcessStdout {
            program,
            args,
            format_hint: _,
        } => {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .wrap_err("failed to create async runtime for process audio streaming")?;
            let reader = runtime
                .block_on(async {
                    let command = CommandBuilder::new(
                        ProcessCommand::new(program.clone()).args(args.clone()),
                    )
                    .pipe(FfmpegConvertAudioCommand::new("wav"));
                    let params = ProcessStreamParams::new(command)
                        .wrap_err("failed to configure process audio pipeline")?;
                    StreamDownload::new_process(
                        params,
                        TempStorageProvider::new(),
                        Settings::default().cancel_on_drop(false),
                    )
                    .await
                    .wrap_err("failed to open process audio stream")
                })
                .wrap_err("failed to open process audio stream")?;
            Ok((Box::new(reader), None, Some(runtime)))
        }
    }
}

fn build_http_client(headers: &[(String, String)]) -> Result<ReqwestClient> {
    let mut default_headers = HeaderMap::new();
    for (name, value) in headers {
        let header_name =
            HeaderName::from_str(name).wrap_err_with(|| format!("invalid header name: {name}"))?;
        let header_value = HeaderValue::from_str(value)
            .wrap_err_with(|| format!("invalid header value for {name}"))?;
        default_headers.insert(header_name, header_value);
    }

    ReqwestClient::builder()
        .default_headers(default_headers)
        .build()
        .wrap_err("failed to build HTTP client for audio stream")
}

/// Uses symphonia's format reader to extract duration from the file's container
/// metadata (e.g. Xing/VBRI header for VBR MP3, or stream info for FLAC/WAV).
/// This is more reliable than rodio's `total_duration()` which only works for CBR.
fn probe_duration_symphonia(path: &str) -> Option<Duration> {
    use symphonia::core::{
        formats::FormatOptions, io::MediaSourceStream, meta::MetadataOptions, probe::Hint,
    };

    let file = std::fs::File::open(path).ok()?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
    {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .ok()?;

    let track = probed
        .format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL)?;

    let n_frames = track.codec_params.n_frames?;
    let sample_rate = track.codec_params.sample_rate?;
    Some(Duration::from_secs_f64(
        n_frames as f64 / sample_rate as f64,
    ))
}
