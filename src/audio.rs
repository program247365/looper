use color_eyre::eyre::{Result, WrapErr};
use rodio::source::SeekError;
use rodio::{ChannelCount, Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, SampleRate, Source};
use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::num::NonZeroUsize;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use stream_download::http::HttpStream;
use stream_download::process::{
    Command as ProcessCommand, CommandBuilder, FfmpegConvertAudioCommand, ProcessStreamParams,
};
use stream_download::storage::adaptive::AdaptiveStorageProvider;
use stream_download::storage::memory::MemoryStorageProvider;
use stream_download::storage::temp::TempStorageProvider;
use stream_download::{Settings, StreamDownload};
use tokio::runtime::Runtime;
use tokio_util::sync::CancellationToken;

use crate::playback_input::PlaybackInput;
use stream_download::http::reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use stream_download::http::reqwest::Client as ReqwestClient;

// Enough for 2048 mono FFT samples even with stereo input (2048 * 2 channels * 2x headroom)
const BUF_CAP: usize = 8192;
const STREAM_STORAGE_BUFFER_BYTES: usize = 16 * 1024 * 1024;

fn stream_storage() -> AdaptiveStorageProvider<MemoryStorageProvider, TempStorageProvider> {
    stream_storage_with_buffer(STREAM_STORAGE_BUFFER_BYTES)
}

fn stream_storage_with_buffer(
    buffer_bytes: usize,
) -> AdaptiveStorageProvider<MemoryStorageProvider, TempStorageProvider> {
    AdaptiveStorageProvider::with_fixed_and_variable(
        MemoryStorageProvider,
        TempStorageProvider::new(),
        NonZeroUsize::new(buffer_bytes).expect("stream storage buffer size must be nonzero"),
    )
}

/// Wraps a Source and copies every sample into a shared ring buffer.
/// Uses try_lock so the audio thread never blocks waiting for the main thread.
pub struct SampleTap<S: Source> {
    inner: S,
    buf: Arc<Mutex<VecDeque<f32>>>,
}

impl<S: Source> Iterator for SampleTap<S> {
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

impl<S: Source> Source for SampleTap<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> ChannelCount {
        self.inner.channels()
    }
    fn sample_rate(&self) -> SampleRate {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)
    }
}

pub struct AudioPlayer {
    // Owns the OS audio output; dropping it stops playback, so keep it alive.
    _device: MixerDeviceSink,
    // Cancels the stream download at teardown (see `Drop` below). Without it a
    // stalled stream (e.g. a live broadcast that went quiet) leaves the audio
    // callback blocked inside `StreamDownload::read`, and dropping `_device`
    // then waits on that callback forever — freezing the whole app on quit.
    download_cancel: Option<CancellationToken>,
    _download_runtime: Option<Runtime>,
    pub sink: Player,
    pub duration: Option<Duration>,
    pub sample_buf: Arc<Mutex<VecDeque<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
    input: PlaybackInput,
    refillable: bool,
    // Keeps librespot playback alive for Spotify sources; dropping it stops the
    // track and its end-of-track loop listener. `None` for all other inputs.
    _spotify: Option<crate::spotify::SpotifyPlayback>,
}

trait MediaReader: Read + Seek + Send + Sync {}

impl<T: Read + Seek + Send + Sync> MediaReader for T {}

impl AudioPlayer {
    pub fn new(input: PlaybackInput, repeat: bool) -> Result<Self> {
        let mut device = DeviceSinkBuilder::open_default_sink()
            .wrap_err("failed to open audio output device")?;
        // rodio prints "Dropping DeviceSink, audio playing through this sink
        // will stop" to stderr on drop, which leaks onto the TUI's alternate
        // screen whenever an AudioPlayer is torn down (track/loop transitions).
        device.log_on_drop(false);
        let sink = Player::connect_new(device.mixer());

        // Spotify can't be read as a file or byte stream: librespot decodes it
        // in-process and pushes PCM through a bridge `Source`. Wire that up and
        // return early — the file/stream decode path below doesn't apply.
        if let PlaybackInput::Spotify { track_uri } = &input {
            let (source, playback, sample_rate, channels, duration) =
                crate::spotify::open_playback(track_uri, repeat)?;
            let buf = Arc::new(Mutex::new(VecDeque::with_capacity(BUF_CAP)));
            sink.append(SampleTap {
                inner: source,
                buf: buf.clone(),
            });
            return Ok(Self {
                _device: device,
                download_cancel: None,
                _download_runtime: None,
                sink,
                duration,
                sample_buf: buf,
                sample_rate,
                channels,
                input,
                refillable: false,
                _spotify: Some(playback),
            });
        }

        let (reader, duration, runtime, byte_len, download_cancel) = open_input(&input)?;
        let source = decode_input(reader, byte_len)?;

        let sample_rate = source.sample_rate().get();
        let channels = source.channels().get();

        // File-backed sources can be cheaply re-opened from disk, so we play
        // them once and refill the sink on each loop boundary instead of using
        // rodio's `repeat_infinite()` — which materializes the entire decoded
        // PCM in RAM via `Buffered`. For long tracks that grows unbounded.
        let refillable = repeat && matches!(input, PlaybackInput::File(_));

        let buf = Arc::new(Mutex::new(VecDeque::with_capacity(BUF_CAP)));
        if repeat && !refillable {
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
            _device: device,
            download_cancel,
            _download_runtime: runtime,
            sink,
            duration,
            sample_buf: buf,
            sample_rate,
            channels,
            input,
            refillable,
            _spotify: None,
        })
    }

    pub fn pause(&self) {
        self.sink.pause();
    }

    pub fn resume(&self) {
        self.sink.play();
    }

    pub fn skip(&self) {
        self.sink.stop();
    }

    pub fn seek_to(&self, position: Duration) -> Result<bool> {
        if !matches!(self.input, PlaybackInput::File(_)) {
            return Ok(false);
        }

        // True seek: jumps to the target packet via symphonia without re-decoding
        // from the start. Blocks ~0-5ms even when paused. On any seek failure we
        // report no-op rather than disrupt playback.
        match self.sink.try_seek(position) {
            Ok(()) => {
                if let Ok(mut buf) = self.sample_buf.lock() {
                    buf.clear();
                }
                Ok(true)
            }
            Err(_) => Ok(false),
        }
    }

    /// If this player loops a file-backed source and the sink has drained,
    /// reopen the file, decode a fresh source, and queue it. Returns true
    /// when a refill happened so the caller can advance its loop counter.
    pub fn try_refill_loop(&self) -> Result<bool> {
        if !self.refillable || !self.sink.empty() {
            return Ok(false);
        }
        let (reader, _, _, byte_len, _) = open_input(&self.input)?;
        let source = decode_input(reader, byte_len)?;
        let tapped = SampleTap {
            inner: source,
            buf: self.sample_buf.clone(),
        };
        self.sink.append(tapped);
        Ok(true)
    }
}

/// Runs before the fields drop. Cancelling the download makes stream-download
/// kill the yt-dlp/ffmpeg children and signal "stream done", which wakes any
/// audio callback blocked inside `StreamDownload::read`. Only then can
/// `_device`'s drop (which waits for the callback to return) complete. The
/// runtime processing the cancellation is still alive here — fields drop after.
impl Drop for AudioPlayer {
    fn drop(&mut self) {
        if let Some(token) = &self.download_cancel {
            token.cancel();
        }
    }
}

fn decode_input(
    reader: Box<dyn MediaReader>,
    byte_len: Option<u64>,
) -> Result<Decoder<BufReader<Box<dyn MediaReader>>>> {
    let reader = BufReader::new(reader);
    let mut builder = Decoder::builder().with_data(reader).with_seekable(true);
    // Byte length lets symphonia seek accurately in VBR files (e.g. long MP3s).
    if let Some(len) = byte_len {
        builder = builder.with_byte_len(len);
    }
    builder.build().wrap_err("failed to decode audio")
}

type OpenedInput = (
    Box<dyn MediaReader>,
    Option<Duration>,
    Option<Runtime>,
    Option<u64>,
    Option<CancellationToken>,
);

fn open_input(input: &PlaybackInput) -> Result<OpenedInput> {
    match input {
        PlaybackInput::File(path) => {
            let path_str = path.to_string_lossy();
            let duration = File::open(path)
                .ok()
                .and_then(|f| Decoder::new(BufReader::new(f)).ok())
                .and_then(|d| d.total_duration())
                .or_else(|| probe_duration_symphonia(&path_str));

            let file = File::open(path).wrap_err("failed to open audio file")?;
            let byte_len = file.metadata().ok().map(|m| m.len());
            Ok((Box::new(file), duration, None, byte_len, None))
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

                    StreamDownload::from_stream(stream, stream_storage(), Settings::default())
                        .await
                        .wrap_err("failed to open HTTP audio stream")
                })
                .wrap_err("failed to open HTTP audio stream")?;
            let cancel = reader.cancellation_token();
            Ok((Box::new(reader), None, Some(runtime), None, Some(cancel)))
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
                        stream_storage(),
                        Settings::default().cancel_on_drop(false),
                    )
                    .await
                    .wrap_err("failed to open process audio stream")
                })
                .wrap_err("failed to open process audio stream")?;
            let cancel = reader.cancellation_token();
            Ok((Box::new(reader), None, Some(runtime), None, Some(cancel)))
        }
        // Spotify is wired up earlier in `AudioPlayer::new` and never reaches
        // the file/stream reader path.
        PlaybackInput::Spotify { .. } => {
            unreachable!("Spotify inputs are handled before open_input")
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

#[cfg(test)]
mod tests {
    use super::stream_storage_with_buffer;
    use std::io::{Read, Write};
    use stream_download::storage::adaptive::{AdaptiveStorageReader, AdaptiveStorageWriter};
    use stream_download::storage::StorageProvider;

    #[test]
    #[ignore = "requires a real audio output device"]
    fn seek_on_local_file_is_true_seek() {
        use crate::playback_input::PlaybackInput;
        use std::time::{Duration, Instant};

        let input = PlaybackInput::file("tests/fixtures/sound.mp3");
        let player = super::AudioPlayer::new(input, true).expect("player should initialize");
        assert!(player.duration.is_some(), "duration should be probed");

        // A true seek returns promptly (~ms). The old skip_duration path would
        // decode-and-discard from zero; this guards against regressing to it.
        let start = Instant::now();
        let seeked = player
            .seek_to(Duration::from_millis(500))
            .expect("seek should not error");
        assert!(seeked, "local file seek should report success");
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "true seek must not block on a full decode"
        );
    }

    #[test]
    #[ignore = "requires a real audio output device and ffmpeg"]
    fn drop_does_not_hang_on_stalled_process_stream() {
        use crate::playback_input::{PlaybackInput, ProcessFormat};
        use std::time::Duration;

        // Emits ~6s of WAV (past the 256 KB stream prefetch), then stalls
        // forever without EOF — the shape of a live stream whose upstream
        // broadcast went quiet.
        let input = PlaybackInput::process_stdout(
            "sh",
            vec![
                "-c".to_string(),
                "ffmpeg -v error -f lavfi -i sine=frequency=200:duration=6 -af volume=0.05 -f wav -; sleep 600"
                    .to_string(),
            ],
            ProcessFormat::Wav,
        );
        let player = super::AudioPlayer::new(input, false).expect("player should initialize");
        // Let playback drain the buffered audio so the decoder is blocked
        // inside the audio callback waiting on the stalled stream.
        std::thread::sleep(Duration::from_secs(9));

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(player);
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("AudioPlayer drop wedged on a stalled stream");
    }

    #[test]
    fn unknown_length_stream_storage_is_bounded() {
        let (mut reader, mut writer) = stream_storage_with_buffer(4)
            .into_reader_writer(None)
            .expect("stream storage should initialize");

        assert!(matches!(reader, AdaptiveStorageReader::Bounded(_)));
        assert!(matches!(writer, AdaptiveStorageWriter::Bounded(_)));

        assert_eq!(writer.write(&[1, 2, 3, 4, 5]).unwrap(), 4);
        assert_eq!(writer.write(&[5]).unwrap(), 0);

        let mut read_buf = [0; 2];
        assert_eq!(reader.read(&mut read_buf).unwrap(), 2);
        assert_eq!(read_buf, [1, 2]);
        assert_eq!(writer.write(&[5, 6]).unwrap(), 2);
    }
}
