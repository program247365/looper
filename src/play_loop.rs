use color_eyre::Result;

pub fn play_file(url: &str) -> Result<()> {
    println!("Playing {:?} on loop", url);

    for _i in 0..=10 {
        play::play(&url)?;
    }

    Ok(())
}
