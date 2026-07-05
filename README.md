# Looper

**A terminal music player that loops anything — local files, [YouTube](https://www.youtube.com), [SoundCloud](https://soundcloud.com), [HypeM](https://hypem.com), and [Spotify](https://www.spotify.com) — with a real-time visualizer and native Now Playing integration for macOS.**

![Looper fullscreen visualizer](screenshots/looper.png)

Drop in a file or paste a URL and looper plays it on repeat in a clean `ratatui`
terminal UI: a live FFT scatter visualizer, album art, history, favorites, and
full macOS/Linux media-key support. Put on a track, hit fullscreen, stay in the
zone.

## Highlights

- **Play from anywhere** — local files, [YouTube](https://www.youtube.com) (incl. live), [SoundCloud](https://soundcloud.com), [HypeM](https://hypem.com), and [Spotify](https://www.spotify.com) tracks, playlists, and albums
- **Real-time visualizer** — a log-spaced FFT scatter with a fullscreen mode (`f`)
- **Loops the way you want** — single tracks forever; playlists and albums play through, then repeat
- **Native Now Playing** — album art, artist, and album in macOS Control Center / lock screen and Linux MPRIS, plus play/pause/next/previous media keys
- **Remembers everything** — SQLite-backed history and favorites, with optional cross-device sync
- **Offline-first and resilient** — a fast local DB, cached downloads, automatic Spotify reconnect, and graceful handling of dead links

## Install

```shell
brew tap program247365/tap
brew install looper
```

Apple Silicon gets a prebuilt binary. For Intel, building from source, or Linux,
see [Installation](docs/installation.md).

## Quick start

```shell
looper                                              # browse your history; Enter to replay
looper play --url ~/music/focus.mp3                 # a local file, on loop
looper play --url "https://www.youtube.com/watch?v=xAR6N9N8e6U"
```

[Spotify](https://www.spotify.com) (Premium) takes a one-time browser login:

```shell
looper spotify login
looper play --url "https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy"
```

Press `f` for fullscreen, `space` to pause, `p` for history, `q` to quit. More
examples and keybindings in [Usage & Keys](docs/usage.md).

Want in-app Spotify search (`/` to find and loop a song, album, or playlist)?
That takes a free Spotify API app of your own — one-time setup in
[Spotify → Search](docs/spotify.md#search-optional).

## Documentation

| Guide | What's inside |
|-------|---------------|
| [Installation](docs/installation.md) | Homebrew, build from source, runtime dependencies |
| [Usage & Keys](docs/usage.md) | Every source, playlists, controls, the history panel |
| [Spotify](docs/spotify.md) | Login, playback, in-app search setup, and how librespot streaming works |
| [Now Playing & Media Keys](docs/now-playing.md) | The OS media-session integration |
| [How Playback Works](docs/playback.md) | The `yt-dlp` model, caching, and quirks |
| [Cross-Device Sync](docs/sync.md) | Share history across machines via a cloud folder |
| [Development](docs/development.md) | Build, test, and release |

## Requirements

- macOS or Linux
- [`yt-dlp`](https://github.com/yt-dlp/yt-dlp) + [`ffmpeg`](https://ffmpeg.org) for YouTube / SoundCloud / HypeM (auto-installed by Homebrew)
- Spotify needs neither — just a Premium account and a one-time login

## License

[MIT](LICENSE) · Built in Rust with [ratatui](https://ratatui.rs) · [kbr.sh/looper](https://kbr.sh/looper)
