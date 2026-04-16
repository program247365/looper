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

/// A CLI tool that plays songs on loop so you can get in the zone
///
/// Visit https://kbr.sh/looper for more!
#[derive(StructOpt, Debug)]
#[structopt(name = "looper")]
struct Opt {
    #[structopt(subcommand)]
    cmd: Option<Command>,
}

#[derive(StructOpt, Debug)]
enum Command {
    /// play something on loop
    ///
    /// This command will play the file you give it
    /// on a loop until you exit the program
    Play {
        /// Optionally play a specific file
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
