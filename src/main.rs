use color_eyre::eyre::Result;
use structopt::StructOpt;

mod audio;
mod download;
mod play_loop;
mod playback_input;
mod plugin;
mod schema;
mod storage;
mod tui;
use play_loop::{browse_history, play_file};

/// A CLI audio looper with a TUI visualizer — play local files, YouTube,
/// SoundCloud, or HypeM tracks and playlists on repeat so you can stay in the zone.
///
/// Run `looper` with no arguments to browse your playback history.
///
/// Requires yt-dlp and ffmpeg for remote playback.
///
/// Visit https://kbr.sh/looper for more!
#[derive(StructOpt, Debug)]
#[structopt(
    name = "looper",
    after_help = "\
EXAMPLES:
    Browse playback history (default):
        looper

    Play a local audio file on loop:
        looper play --url ~/music/focus.mp3

    Play a YouTube video on loop:
        looper play --url https://www.youtube.com/watch?v=dQw4w9WgXcQ

    Play an entire YouTube playlist:
        looper play --url https://www.youtube.com/playlist?list=PLrAXtmErZgOeiKm4sgNOknGvNjby9efdf

    Play a SoundCloud track:
        looper play --url https://soundcloud.com/artist/track-name

    Play a HypeM track:
        looper play --url https://hypem.com/track/2d8a0/

SUPPORTED SOURCES:
    Local files    .mp3, .wav, .flac, .ogg, and other formats supported by symphonia
    YouTube        Single videos and playlists
    SoundCloud     Single tracks and playlists/sets
    HypeM          Individual tracks

PLAYBACK BEHAVIOR:
    Single track   Loops forever until you quit
    Playlist       Plays each track once, then loops the whole playlist

TUI CONTROLS:
    q / Ctrl-C     Quit
    Space          Pause / resume
    f              Toggle fullscreen visualizer
    s              Toggle favorite
    p / Esc        Toggle history panel

  History panel:
    j / k          Navigate up / down
    h / l          Change sort column
    r              Reverse sort order
    s              Toggle favorite
    Enter          Replay selected track"
)]
struct Opt {
    #[structopt(subcommand)]
    cmd: Option<Command>,
}

#[derive(StructOpt, Debug)]
enum Command {
    /// Play a local file or remote URL on loop
    ///
    /// Accepts local file paths, YouTube URLs (videos and playlists),
    /// SoundCloud URLs (tracks and playlists), and HypeM track URLs.
    ///
    /// Remote tracks are cached locally after the first download.
    /// Playlists are prefetched in the background for gapless playback.
    #[structopt(
        after_help = "\
EXAMPLES:
    looper play --url ~/music/focus.mp3
    looper play --url https://www.youtube.com/watch?v=dQw4w9WgXcQ
    looper play --url https://soundcloud.com/artist/track-name"
    )]
    Play {
        /// Path to a local audio file or a remote URL (YouTube, SoundCloud, HypeM)
        #[structopt(short, long)]
        url: String,
    },
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let opt = Opt::from_args();

    match opt.cmd {
        Some(Command::Play { url }) => play_file(&url),
        None => browse_history(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_looper_as_history_browser() {
        let opt = Opt::from_iter_safe(["looper"]).expect("bare looper should parse");
        assert!(opt.cmd.is_none());
    }

    #[test]
    fn parses_explicit_play_command() {
        let opt = Opt::from_iter_safe(["looper", "play", "--url", "sound.mp3"])
            .expect("play command should parse");
        assert!(matches!(
            opt.cmd,
            Some(Command::Play { ref url }) if url == "sound.mp3"
        ));
    }
}
