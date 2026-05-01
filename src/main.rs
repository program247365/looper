use color_eyre::eyre::Result;
use std::sync::mpsc;
use structopt::StructOpt;

mod audio;
mod download;
#[cfg(target_os = "macos")]
mod macos_runloop;
mod media_controls;
mod play_loop;
mod playback_input;
mod plugin;
mod schema;
mod storage;
mod tui;
use play_loop::{browse_history, play_file, PlaybackContext};

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
    n              Next track (playlist mode)
    b              Previous track (playlist mode)
    f              Toggle fullscreen visualizer
    s              Toggle favorite
    p / Esc        Toggle history panel

  macOS only — media keys (works while TUI is in the background):
    Play/Pause     Toggle pause
    Next           Skip to next track in playlist
    Previous       Skip to previous track in playlist
    Track info also appears in the system Now Playing widget
    (Control Center, lock screen, AirPods controls).

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
    /// Configure looper settings
    Config {
        #[structopt(subcommand)]
        cmd: ConfigCmd,
    },
}

#[derive(StructOpt, Debug)]
enum ConfigCmd {
    /// Set a configuration value
    Set {
        #[structopt(subcommand)]
        key: ConfigKey,
    },
    /// Show current configuration
    Show,
}

#[derive(StructOpt, Debug)]
enum ConfigKey {
    /// Set the folder where looper.sqlite3 is stored (e.g. a Dropbox or custom path).
    /// Overrides iCloud auto-detection. Run once per machine.
    SyncFolder { path: String },
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let opt = Opt::from_args();

    run_app(opt)
}

#[cfg(target_os = "macos")]
fn run_app(opt: Opt) -> Result<()> {
    // On macOS, MPRemoteCommandCenter callbacks dispatch from the main thread's
    // AppKit run loop. Create the media session here, then move the TUI work
    // onto a worker thread. The main thread runs NSApp.run() until the worker
    // exits the process.
    let (session, cmd_rx) = match media_controls::MediaSession::start() {
        Ok((s, rx)) => (Some(s), rx),
        Err(err) => {
            eprintln!("looper: media controls unavailable: {err}");
            (None, mpsc::channel::<play_loop::KeyCommand>().1)
        }
    };
    let media_handle = session.as_ref().map(|s| s.handle());
    // Keep the session alive for the lifetime of the process.
    let _session = session;

    macos_runloop::run_with_tui_thread(move || {
        let ctx = PlaybackContext {
            cmd_rx: &cmd_rx,
            media: media_handle.clone(),
        };
        let result = match opt.cmd {
            Some(Command::Play { url }) => play_file(&url, ctx),
            Some(Command::Config { cmd }) => cmd_config(cmd),
            None => browse_history(ctx),
        };
        match result {
            Ok(()) => 0,
            Err(err) => {
                eprintln!("{err:?}");
                1
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
fn run_app(opt: Opt) -> Result<()> {
    // Linux: souvlaki spawns its own DBus thread; the main thread stays free
    // for the TUI. Windows: deferred (would need a hidden HWND + message pump).
    #[cfg(target_os = "linux")]
    let (session, cmd_rx) = match media_controls::MediaSession::start() {
        Ok((s, rx)) => (Some(s), rx),
        Err(err) => {
            eprintln!("looper: media controls unavailable: {err}");
            (None, mpsc::channel::<play_loop::KeyCommand>().1)
        }
    };
    #[cfg(not(target_os = "linux"))]
    let (session, cmd_rx): (Option<media_controls::MediaSession>, _) =
        (None, mpsc::channel::<play_loop::KeyCommand>().1);

    let media_handle = session.as_ref().map(|s| s.handle());
    let _session = session;

    let ctx = PlaybackContext {
        cmd_rx: &cmd_rx,
        media: media_handle,
    };
    match opt.cmd {
        Some(Command::Play { url }) => play_file(&url, ctx),
        Some(Command::Config { cmd }) => cmd_config(cmd),
        None => browse_history(ctx),
    }
}

fn cmd_config(cmd: ConfigCmd) -> Result<()> {
    match cmd {
        ConfigCmd::Set { key: ConfigKey::SyncFolder { path } } => {
            storage::write_sync_folder_config(std::path::Path::new(&path))?;
            println!("Sync folder set to: {path}");
            println!("looper will use this folder for looper.sqlite3 on next launch.");
        }
        ConfigCmd::Show => match storage::read_sync_folder_config() {
            Some(folder) => println!("sync_folder = {}", folder.display()),
            None => println!(
                "sync_folder = (auto — iCloud Drive if available, otherwise platform default)"
            ),
        },
    }
    Ok(())
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
