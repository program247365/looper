use color_eyre::eyre::Result;
use structopt::StructOpt;

mod play_loop;
use play_loop::play_file;

/// A CLI tool that plays songs on loop so you can get in the zone
///
/// Visit https://kbr.sh/looper for more!
#[derive(StructOpt, Debug)]
#[structopt(name = "looper")]
struct Opt {
    #[structopt(subcommand)]
    cmd: Command,
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
    // dbg!(&opt);
    match opt.cmd {
        Command::Play { url } => play_file(&url),
    }
}
