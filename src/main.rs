use std::path::PathBuf;

use color_eyre::eyre::{eyre, Result, WrapErr};
use directories::UserDirs;
use looper::play_file;
use structopt::StructOpt;

/// A CLI tool that plays songs on loop so you can get in the zone
///
/// Visit https://kbr.sh/looper for more!
#[derive(StructOpt, Debug)]
#[structopt(name = "looper")]
struct Opt {
    #[structopt(parse(from_os_str), short = "p", long, env)]
    looper_path: Option<PathBuf>,

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
        url: Option<String>,
    },
}

fn get_default_looper_dir() -> Result<PathBuf> {
    let user_dirs = UserDirs::new().ok_or_else(|| eyre!("Could not find home directory"))?;
    Ok(user_dirs.home_dir().join(".looper"))
}

fn main() -> Result<()> {
    color_eyre::install()?;
    let opt = Opt::from_args();
    let looper_path = match opt.looper_path {
        Some(pathbuf) => Ok(pathbuf),
        None => get_default_looper_dir().wrap_err("`garden_path` was not supplied"),
    }?;
    // dbg!(&opt);
    match opt.cmd {
        Command::Play { url } => play_file(&looper_path, url),
    }
}
