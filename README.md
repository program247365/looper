# Looper

> A CLI tool that plays a song on loop so you can get in the zone — with a real-time FFT visualizer.

![Looper fullscreen visualizer](screenshots/looper.png)

## Install

### Homebrew (recommended)

```shell
brew tap program247365/tap
brew install looper
```

### Build from source

```shell
git clone https://github.com/program247365/looper.git
cd looper
make install   # builds release binary and installs to /usr/local/bin
```

Requires Rust. Install via [rustup](https://rustup.rs) if needed.

## Usage

```shell
looper play --url "/path/to/your/song.mp3"
```

### Keys

| Key | Action |
|-----|--------|
| `Space` | Pause / Resume |
| `f` | Toggle fullscreen visualizer |
| `q` / `Ctrl-C` | Quit |

## Development

```shell
make run          # play fixture file (tests/fixtures/sound.mp3)
make test         # run tests
make build        # debug build
make build-release # optimized release binary
```

### Releasing a new version

```shell
make release-patch   # 0.1.0 → 0.1.1: bump, tag, push, GH release, update Homebrew formula
make release-minor   # 0.1.x → 0.2.0
```
