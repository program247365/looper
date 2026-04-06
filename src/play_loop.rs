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
use crate::tui::{AppState, N_BANDS, draw, restore_terminal, setup_terminal};

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
        bands: vec![0.0; N_BANDS],
        prev_bands: vec![0.0; N_BANDS],
        fullscreen: false,
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
        if !state.paused {
            update_visualizer(state, player);
        }

        terminal.draw(|f| draw(f, state))?;

        if event::poll(Duration::from_millis(50))? {
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
                    (KeyCode::Char('f'), _) => {
                        state.fullscreen = !state.fullscreen;
                    }
                    _ => {}
                }
            }
        }

        // Advance loop counter when elapsed time exceeds song duration.
        // We track time ourselves rather than Sink::get_pos() because
        // repeat_infinite() doesn't reset the sink position between loops.
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

/// Reads the latest samples from the audio tap, runs FFT via spectrum-analyzer,
/// maps to N_BANDS logarithmically-spaced frequency bands, and applies
/// asymmetric smoothing (fast attack, slow decay) for visual stability.
fn update_visualizer(state: &mut AppState, player: &AudioPlayer) {
    use spectrum_analyzer::windows::hann_window;
    use spectrum_analyzer::{FrequencyLimit, samples_fft_to_spectrum};

    const FFT_LEN: usize = 2048;

    // Grab the most recent samples. We need FFT_LEN mono samples, which means
    // FFT_LEN * channels raw (interleaved) samples from the ring buffer.
    let needed = FFT_LEN * player.channels as usize;
    let raw: Vec<f32> = {
        let buf = player.sample_buf.lock().unwrap();
        if buf.len() < needed {
            return; // not enough data yet (first few frames)
        }
        let start = buf.len() - needed;
        buf.iter().skip(start).cloned().collect()
    };

    // Down-mix interleaved stereo → mono by averaging channel pairs
    let mono: Vec<f32> = if player.channels == 2 {
        raw.chunks_exact(2)
            .map(|c| (c[0] + c[1]) * 0.5)
            .collect()
    } else {
        raw
    };

    let windowed = hann_window(&mono[..FFT_LEN]);
    let spectrum = match samples_fft_to_spectrum(
        &windowed,
        player.sample_rate,
        FrequencyLimit::Range(20.0, 20_000.0),
        None,
    ) {
        Ok(s) => s,
        Err(_) => return,
    };

    // Map spectrum bins into N_BANDS logarithmically-spaced bands (20 Hz – 20 kHz)
    for i in 0..N_BANDS {
        let f_lo = 20.0_f32 * (1000.0_f32).powf(i as f32 / N_BANDS as f32);
        let f_hi = 20.0_f32 * (1000.0_f32).powf((i + 1) as f32 / N_BANDS as f32);

        let vals: Vec<f32> = spectrum
            .data()
            .iter()
            .filter(|(f, _)| f.val() >= f_lo && f.val() < f_hi)
            .map(|(_, v)| v.val())
            .collect();

        let raw_mag = if vals.is_empty() {
            0.0
        } else {
            vals.iter().sum::<f32>() / vals.len() as f32
        };

        // Scale into a visible range and clamp; tune the multiplier if bars feel too short/tall
        let scaled = (raw_mag * 8.0).min(1.0);

        // Asymmetric smoothing: fast attack (responsive to beats), slow decay (smooth falloff)
        state.bands[i] = if scaled > state.prev_bands[i] {
            0.6 * scaled + 0.4 * state.prev_bands[i]
        } else {
            0.25 * scaled + 0.75 * state.prev_bands[i]
        };
        state.prev_bands[i] = state.bands[i];
    }
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
