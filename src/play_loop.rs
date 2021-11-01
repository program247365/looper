use color_eyre::Result;
use std::path::Path;

pub fn play_file(url: &str) -> Result<()> {
    print_playing_filename(&url);

    for _i in 0.. {
        play::play(&url)?;
    }

    Ok(())
}

fn print_playing_filename(url: &str) -> Option<()> {
    let path = Path::new(url);
    let filename = path.file_name()?.to_str()?;
    println!("Playing {} on loop", filename);
    None
}
