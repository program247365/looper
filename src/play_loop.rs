#[cfg(unix)]
use libc;
use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{LeaveAlternateScreen, disable_raw_mode},
};
use std::{
    io::stdout,
    path::Path,
    time::{Duration, Instant},
};

use crate::audio::AudioPlayer;
use crate::tui::{AppState, draw, restore_terminal, setup_terminal};

pub fn play_file(url: &str) -> Result<()> {
    #[cfg(unix)]
    reattach_stdin_to_tty()?;

    let filename = extract_filename(url);
    let player = AudioPlayer::new(url)?;

    let mut state = AppState {
        filename,
        duration: player.duration,
        paused: false,
        loop_count: 1,
        loop_start: Instant::now(),
    };

    let mut terminal = setup_terminal()?;

    // Wrap color_eyre's panic hook so the terminal is restored before the
    // panic message is printed.
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        orig_hook(info);
    }));

    let result = run_loop(&mut terminal, &mut state, &player);

    restore_terminal(&mut terminal)?;
    result
}

fn run_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    state: &mut AppState,
    player: &AudioPlayer,
) -> Result<()> {
    loop {
        terminal.draw(|f| draw(f, state))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        break;
                    }
                    (KeyCode::Char(' '), _) => {
                        if state.paused {
                            player.resume();
                        } else {
                            player.pause();
                        }
                        state.paused = !state.paused;
                    }
                    _ => {}
                }
            }
        }

        // Advance loop counter when the current loop's elapsed time exceeds the
        // song duration. We track time ourselves rather than relying on
        // Sink::get_pos() because repeat_infinite() doesn't reset the sink
        // position between loops.
        if !state.paused {
            if let Some(dur) = state.duration {
                if state.loop_start.elapsed() >= dur {
                    state.loop_count += 1;
                    state.loop_start = Instant::now();
                }
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
fn reattach_stdin_to_tty() -> Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        let path = std::ffi::CString::new("/dev/tty").unwrap();
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDONLY) };
        if fd < 0 {
            color_eyre::eyre::bail!(
                "looper requires a terminal; stdin is not a TTY and /dev/tty could not be opened"
            );
        }
        unsafe {
            libc::dup2(fd, libc::STDIN_FILENO);
            libc::close(fd);
        }
    }
    Ok(())
}

fn extract_filename(url: &str) -> String {
    Path::new(url)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(url)
        .to_string()
}
