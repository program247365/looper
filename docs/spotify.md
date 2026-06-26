[← Back to README](../README.md)

# Spotify

Spotify requires a **Spotify Premium** account.

## Setup

Authorize looper once via your browser:

```shell
looper spotify login
```

This opens Spotify's authorization page, then caches reusable credentials so you
won't need to log in again. No password is stored.

## Playing

```shell
looper play --url "https://open.spotify.com/track/4PTG3Z6ehGkBFwjybzWkR8"
looper play --url "https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M"
looper play --url "https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy"
```

`spotify:` URIs work too. Spotify does **not** require `yt-dlp` or `ffmpeg`.

## How Spotify playback works

Spotify is fundamentally different from the `yt-dlp`-backed services. It exposes
no downloadable audio, so looper uses
[librespot](https://github.com/librespot-org/librespot) — the open-source
implementation of the Spotify Connect protocol — to play full tracks. This is
the same library [spotify-player](https://github.com/aome510/spotify-player)
uses, and it has real constraints:

- **Premium is required.** librespot authenticates as a Spotify Connect device;
  free accounts can't stream through it.
- **OAuth login, once.** `looper spotify login` runs an OAuth browser flow and
  caches reusable credentials under the cache directory.
- **Audio is decoded in-process.** The DRM Ogg/Vorbis stream is decrypted and
  decoded by librespot, and its PCM is bridged straight into looper's audio
  pipeline and FFT visualizer. There is no MP3 on disk like the other services.
- **Looping** re-loads the track when it ends (single track) or advances to the
  next track and loops the whole collection (playlist/album).
- **Album art** is fetched from Spotify's public image CDN and shown in the
  visualizer and the OS [Now Playing](now-playing.md) widget.
- **Connection resilience:** librespot's session doesn't auto-reconnect, so
  looper rebuilds it from cached credentials when it goes stale (sleep/wake,
  network change). Playback recovers at the next track rather than failing.
- **Caveats:** there's a brief loading screen between playlist tracks (no
  background prefetch for Spotify), and an unavailable track (removed or
  region-locked) shows the "track unavailable" modal instead of playing. A
  single track looping forever won't recover from a mid-loop disconnect until
  restart.

Note: librespot is a reverse-engineered client. Using it is for personal use and
is technically outside Spotify's Terms of Service.
