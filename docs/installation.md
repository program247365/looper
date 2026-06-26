[← Back to README](../README.md)

# Installation

## Homebrew (Apple Silicon)

```shell
brew tap program247365/tap
brew install looper
```

Upgrade an existing install:

```shell
brew update
brew upgrade program247365/tap/looper
```

The Homebrew formula ships a prebuilt binary for `aarch64-apple-darwin`, so
install and upgrade are a small download and a file move — no compile step on
your machine. `ffmpeg` and `yt-dlp` are pulled in automatically as runtime
dependencies.

Intel macOS users: there is no prebuilt binary. Build from source (below) or run
`brew install --HEAD program247365/tap/looper` to compile from `main`.

## Build from source

```shell
git clone https://github.com/program247365/looper.git
cd looper
make install
```

Requires Rust — install via [rustup](https://rustup.rs) if needed.

## Runtime dependencies

YouTube, SoundCloud, and HypeM playback need [`yt-dlp`](https://github.com/yt-dlp/yt-dlp)
and [`ffmpeg`](https://ffmpeg.org):

```shell
brew install yt-dlp ffmpeg
```

If YouTube playback starts failing with `403` errors, update `yt-dlp` first.

Spotify needs **neither** `yt-dlp` nor `ffmpeg` — it decodes in-process via
librespot. It only needs a Spotify Premium account and a one-time login. See
[Spotify](spotify.md).
