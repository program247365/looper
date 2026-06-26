[← Back to README](../README.md)

# Now Playing & Media Keys

looper registers with the OS media session, so whatever it's playing shows up in
the system **Now Playing** surface — macOS Control Center, the lock screen, and
AirPods/keyboard controls; Linux MPRIS clients — with full metadata:

| Field | Where it comes from |
|-------|---------------------|
| Title | the track title |
| Artist | the real artist when the source provides one (Spotify, and YouTube/SoundCloud/HypeM via `yt-dlp`); falls back to the service name |
| Album | the playlist or album name, when playing one |
| Artwork | the Spotify cover, the YouTube/SoundCloud/HypeM thumbnail, or — for local files, which have no embedded art — a bundled fallback cover so the widget is never blank |

Media keys work even while looper is in the background:

| Key | Action |
|-----|--------|
| Play / Pause | Toggle pause |
| Next | Skip to the next track (playlist mode) |
| Previous | Skip to the previous track (playlist mode) |

On macOS this uses `MPRemoteCommandCenter` + `MPNowPlayingInfoCenter`; on Linux,
MPRIS over D-Bus. Windows is not wired up.
