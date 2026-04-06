use color_eyre::eyre::{Result, WrapErr};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::VecDeque;
use std::fs::File;
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
    pub sink: Sink,
    pub duration: Option<Duration>,
    pub sample_buf: Arc<Mutex<VecDeque<f32>>>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl AudioPlayer {
    pub fn new(path: &str) -> Result<Self> {
        let (stream, handle) =
            OutputStream::try_default().wrap_err("failed to open audio output device")?;
        let sink = Sink::try_new(&handle).wrap_err("failed to create audio sink")?;

        // Probe duration: try rodio's decoder first (works for CBR), fall back to
        // symphonia's format probe which reads the Xing/VBRI header for VBR MP3s.
        let duration = File::open(path)
            .ok()
            .and_then(|f| Decoder::new(BufReader::new(f)).ok())
            .and_then(|d| d.total_duration())
            .or_else(|| probe_duration_symphonia(path));

        // Playback decoder — convert to f32 so SampleTap works with FFT directly
        let file = File::open(path).wrap_err("failed to open audio file")?;
        let source = Decoder::new(BufReader::new(file))
            .wrap_err("failed to decode audio file")?
            .convert_samples::<f32>()
            .repeat_infinite();

        let sample_rate = source.sample_rate();
        let channels = source.channels();

        let buf = Arc::new(Mutex::new(VecDeque::with_capacity(BUF_CAP)));
        let tapped = SampleTap {
            inner: source,
            buf: buf.clone(),
        };
        sink.append(tapped);

        Ok(Self {
            _stream: stream,
            _stream_handle: handle,
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
