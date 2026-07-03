[← Back to README](../README.md)

# Usage & Keys

## Default startup

```shell
looper
```

Opens the playlist-history browser with no active playback. Press `Enter` on a
row to start playing it. To skip the browser and jump straight into playback,
use `looper play --url ...`.

## Local files

```shell
looper play --url "/path/to/your/song.mp3"
```

## YouTube

```shell
looper play --url "https://www.youtube.com/watch?v=xAR6N9N8e6U"
```

### YouTube Live

Livestream URLs are detected automatically and played as a continuous stream
(no full download, no loop counter — a red `● STREAMING` badge replaces the
usual `● PLAYING` label).

```shell
looper play --url "https://www.youtube.com/watch?v=YmQ7jRgf4f0"
```

That example is Anthropic's Claude FM channel — music for thinking and building.

## SoundCloud

```shell
looper play --url "https://soundcloud.com/odesza/line-of-sight-feat-wynne-mansionair"
```

## HypeM

```shell
looper play --url "https://hypem.com/track/2gq0d/CHVRCHES+-+Clearest+Blue"
```

## Playlists

```shell
looper play --url "https://www.youtube.com/playlist?list=PLFgquLnL59alCl_2TQvOiD5Vgm1hCaGSI"
```

Single tracks loop forever; playlists and albums play each track once, then loop
the whole collection.

## Spotify

Spotify needs a one-time browser login and a Premium account. See
[Spotify](spotify.md) for setup and details.

```shell
looper spotify login
looper play --url "https://open.spotify.com/album/4aawyAB9vmqN3uQ7FjRGTy"
```

## Controls

| Key | Action |
|-----|--------|
| `Enter` | Replay the selected track from the history browser |
| `Space` | Pause / resume |
| `Left` / `Right` | Seek backward / forward 5 seconds |
| Click / drag progress bar | Scrub to any position (commits on release) |
| `n` / `b` | Next / previous track (playlist mode) |
| `f` | Toggle fullscreen visualizer |
| `s` | Toggle favorite for the current track |
| `p` / `Esc` | Toggle the played-songs panel |
| `/` | Open Spotify search |
| `q` / `Ctrl-C` | Quit |

Seeking (arrow keys or progress-bar drag) is available for local files and
cached downloads. Live or stream-first sources ignore seek input.

Hardware/OS media keys (play/pause/next/previous) also work while looper is in
the background — see [Now Playing & Media Keys](now-playing.md).

## Played-songs panel

Bare `looper` opens directly into playlist history. During playback the panel is
hidden by default and opens over the minimal UI.

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up |
| `h` / `l` | Change sort field |
| `r` | Reverse sort direction |
| `s` | Toggle favorite for the selected row |
| `Enter` | Replay the selected track |
| `p` / `Esc` | Close the panel |

Sort fields: time played, last played, platform, title, times played.

## Spotify search

`/` opens a Spotify catalog search from the playback screen or the history
browser (requires the one-time `looper spotify login`). Type a query and press
`Enter`; results are grouped into SONGS, ALBUMS, and PLAYLISTS.

| Key | Action |
|-----|--------|
| `j` / `k` | Move selection down / up |
| `gg` / `G` | Jump to first / last result |
| `/` | Edit the query again |
| `Enter` | Play the selection (song loops; album/playlist plays through) |
| `Esc` | Close search |

While the search overlay is open it captures all keys — `q` types a letter
instead of quitting (`Ctrl-C` still works).
