use std::path::Path;

use color_eyre::Result;

pub fn play_file(url: Option<String>) -> Result<()> {
    let printable_filename = match &url {
        Some(url) => {
            let user_provided_filepath = url.clone();
            let filepath = user_provided_filepath;
            let filename = Path::new(&filepath).file_name();
            filename.unwrap().to_str();
        }
        None => {
            println!("Sorry you need to provide a url to play.");
        }
    };

    println!("Playing {:?} on loop", printable_filename);

    let mut n = 1;
    while n <= 10 {
        if let Some(file) = &url {
            play::play(&file)?;
        }
        n += 1;
    }

    Ok(())
}
