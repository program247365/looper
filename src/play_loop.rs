use std::path::Path;

use color_eyre::Result;

pub fn play_file(url: &str) -> Result<()> {
    let printable_filename = Path::new(&url).file_name().unwrap().to_str();
    println!("Playing {:?} on loop", printable_filename);

    let mut n = 1;
    while n <= 10 {
        play::play(&url)?;
        n += 1;
    }

    Ok(())
}
