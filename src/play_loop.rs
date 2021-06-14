use std::{path::{Path, PathBuf}, string};

use color_eyre::Result;

pub fn play_file(looper_path: &PathBuf, _url: Option<String>) -> Result<()> {
	// let filepath = "./Users/kevin/Music/iTunes/iTunes Media/Music/Chevelle/The North Corridor/1-03 Joyride (Omen).mp3";
	let filepath2 = "/Users/kevin/Music/Downloaded by MediaHuman/Fabrizio Paterlini - Rue des trois frères.mp3";
	let filename = Path::new(filepath2).file_name();
	let printable_filename: String = filename.unwrap().to_str().unwrap().into();
	println!("Playing {} on loop", printable_filename);
	let mut n = 1;
	while n <= 10  {
		play::play(filepath2).unwrap();
		// play::play("./Users/kevin/Code/looper/tests/fixtures/sound.mp3").unwrap();
		n+=1;
	}
	Ok(())
}

/*
    let os_str = OsStr::new("example.txt");
    let path = Path::new(os_str);
    let extensioner = path.extension();
    let my_new_string: String = extensioner.unwrap().to_str().unwrap().into();

*/