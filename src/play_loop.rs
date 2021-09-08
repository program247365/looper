use std::path::Path;

use color_eyre::Result;

pub fn play_file(url: Option<String>) -> Result<()> {
    let printable_filename = match url {
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
        // TODO: figure out how to use this proper and passing the path to the play method when it was a Option<String>
        /*
         * When running this: cargo run play --url /Users/kevin/Code/looper/tests/fixtures/sound.mp3
         * compiling looper v0.1.0 (/Users/kevin/Code/looper)
         * error[E0620]: cast to unsized type: `Option<String>` as `dyn AsRef<Path>`
         *   --> src/play_loop.rs:24:20
         *      |
         *      24 |         play::play(url as dyn AsRef<Path>)?;
         *         |                    ^^^^^^^^^^^^^^^^^^^^^^
         *            |
         *            help: consider using a box or reference as appropriate
         *              --> src/play_loop.rs:24:20
         *                 |
         *                 24 |         play::play(url as dyn AsRef<Path>)?;
         *                    |                    ^^^
         *
         *                    error: aborting due to previous error
         *
         *                    For more information about this error, try `rustc --explain E0620`.
         *                    error: could not compile `looper`))
         *
         */
        play::play(url as dyn AsRef<Path>)?;
        n += 1;
    }

    Ok(())
}
