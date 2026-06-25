//! The push/pull bridge between librespot and rodio.
//!
//! librespot's `Player` *pushes* decoded PCM into an audio-backend `Sink` on
//! its own thread; rodio *pulls* samples from a `Source` on the audio output
//! thread. We connect them with a bounded channel:
//!
//! - [`SpotifySink`] is librespot's backend. Its `write` sends one buffer per
//!   decoded packet and **blocks when the channel is full** — which throttles
//!   librespot to the audio device's real-time consumption rate (free
//!   backpressure, no manual rate-limiting).
//! - [`SpotifySource`] is the rodio `Source`. On underrun (decoder briefly
//!   behind, or the gap while a track reloads for the next loop) it yields
//!   silence rather than ending. Track end is signalled out-of-band by
//!   `PlayerEvent::EndOfTrack`, never by the sample stream running dry — so
//!   returning `None` here would wrongly end playback on a momentary stall.

use std::collections::VecDeque;
use std::num::NonZero;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError};
use std::time::Duration;

use librespot_playback::audio_backend::{Sink, SinkError, SinkResult};
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use rodio::{ChannelCount, SampleRate, Source};

/// Spotify Ogg/Vorbis streams decode to 44.1 kHz interleaved stereo.
pub const SPOTIFY_SAMPLE_RATE: u32 = 44_100;
pub const SPOTIFY_CHANNELS: u16 = 2;

/// Buffers in flight between the decoder and the audio thread. Each buffer is
/// one decoded packet (a few thousand samples), so a small count bounds added
/// latency to tens of milliseconds while leaving the decoder room to run ahead.
const CHANNEL_CAPACITY: usize = 8;

/// Create a connected `(Sink, Source)` pair sharing one bounded channel.
pub fn bridge() -> (SpotifySink, SpotifySource) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<f32>>(CHANNEL_CAPACITY);
    (SpotifySink { tx }, SpotifySource::new(rx))
}

/// librespot audio backend: forwards decoded PCM into the channel.
pub struct SpotifySink {
    tx: SyncSender<Vec<f32>>,
}

impl Sink for SpotifySink {
    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        // `passthrough` is off, so packets are always decoded f64 samples.
        let samples = match packet.samples() {
            Ok(samples) => samples,
            Err(_) => return Ok(()),
        };
        if samples.is_empty() {
            return Ok(());
        }
        let buf: Vec<f32> = samples.iter().map(|&s| s as f32).collect();
        // Blocks under backpressure. A send error means the `SpotifySource`
        // was dropped (playback torn down) — tell librespot to stop.
        self.tx
            .send(buf)
            .map_err(|_| SinkError::OnWrite("looper audio sink closed".to_string()))
    }
}

/// rodio source pulling PCM from the channel the sink fills.
pub struct SpotifySource {
    rx: Receiver<Vec<f32>>,
    current: VecDeque<f32>,
}

impl SpotifySource {
    fn new(rx: Receiver<Vec<f32>>) -> Self {
        Self {
            rx,
            current: VecDeque::new(),
        }
    }
}

impl Iterator for SpotifySource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        if let Some(sample) = self.current.pop_front() {
            return Some(sample);
        }
        match self.rx.try_recv() {
            Ok(buf) => {
                self.current = VecDeque::from(buf);
                // A sink never sends an empty buffer, but guard anyway.
                Some(self.current.pop_front().unwrap_or(0.0))
            }
            // Underrun, or the brief gap between loop iterations: emit silence.
            // Never `None` — looping/track-end is driven by player events.
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => Some(0.0),
        }
    }
}

impl Source for SpotifySource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        NonZero::new(SPOTIFY_CHANNELS).expect("channel count is nonzero")
    }
    fn sample_rate(&self) -> SampleRate {
        NonZero::new(SPOTIFY_SAMPLE_RATE).expect("sample rate is nonzero")
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}
