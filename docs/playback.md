[← Back to README](../README.md)

# How Playback Works

This covers the `yt-dlp`-backed services (YouTube, SoundCloud, HypeM) and local
files. Spotify is different — see [Spotify](spotify.md).

## The flow

- startup opens the local SQLite database, runs embedded migrations, and then
  begins loading playback
- `yt-dlp` extracts track metadata and media URLs
- remote audio is cached locally (see [Data and cache locations](#data-and-cache-locations))
- uncached remote tracks show a full-screen loading scene before playback
- single tracks loop forever
- playlists play each track once, then loop the entire playlist
- background prefetch caches upcoming playlist tracks when possible

Current behavior is intentionally pragmatic:

- YouTube uses a download-first cached path for reliability
- YouTube livestreams (`live_status: is_live`) are auto-detected and routed to a
  stream-first path with looping disabled; scheduled-but-not-yet-live URLs fail
  fast with a helpful message instead of hanging
- SoundCloud and HypeM prefer a stream-first path and fall back to cached
  download when needed

## Data and cache locations

Remote tracks are cached locally after download:

| Platform | Cache directory |
|----------|----------------|
| macOS | `~/Library/Caches/sh.kbr.looper/` |
| Linux | `~/.cache/looper/` |

Spotify keeps its cached credentials, encrypted audio cache, and album art in a
`spotify/` subfolder of the cache directory above.

Playback history and favorites live in a SQLite database (`looper.sqlite3`).
Where it lives depends on your sync setup — see [Cross-device sync](sync.md).

- startup applies pending embedded migrations automatically — no manual steps
  needed when upgrading
- bare `looper` loads this history first and lets you replay from it
- history is tracked per playable URL or canonical local file path
- each track stores title, platform, favorite state, last played timestamp, play
  count, cumulative time played, and which computer played it last

## Notes & quirks

- Public online URLs work best. Private, age-restricted, or members-only content
  may still fail depending on `yt-dlp` access.
- If a YouTube watch URL includes both `v=` and `list=`, looper currently
  normalizes it toward single-video playback unless you use the playlist URL
  directly.
- The remote loading UI is designed to hand off into playback cleanly rather
  than waiting on a full silent download.
- A dead replay target (private/removed/region-locked) is non-fatal: looper
  shows a "track unavailable" modal and returns to the history browser.
- The startup screen and loading copy are intentionally a little cheeky.
