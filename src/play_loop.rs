use color_eyre::eyre::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, LeaveAlternateScreen},
};
#[cfg(unix)]
use libc;
use std::{
    collections::HashSet,
    io::{stdout, Write},
    path::Path,
    sync::{
        mpsc::{self, Receiver, SyncSender, TrySendError},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

use crate::audio::AudioPlayer;
use crate::download::{DownloadEvent, LoadingPhase, LoadingState};
use crate::playback_input::PlaybackInput;
use crate::plugin::{self, ytdlp, TrackInfo};
use crate::storage::{track_record, HistorySortField, SharedStorage, Storage};
use crate::tui::{
    draw, draw_loading, draw_startup, restore_terminal, setup_terminal, AppState,
    HistoryPanelState, StartupScreenState, N_BANDS,
};

pub fn play_file(url: &str) -> Result<()> {
    #[cfg(unix)]
    reattach_stdin_to_tty()?;

    let mut terminal = setup_terminal()?;
    let mut title_state = TitleState::new();

    // Wrap color_eyre's panic hook so the terminal is restored before the
    // panic message is printed.
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        orig_hook(info);
    }));

    let result = play_file_session(&mut terminal, &mut title_state, url);

    let _ = title_state.reset();
    restore_terminal(&mut terminal)?;
    result
}

fn play_file_session(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    initial_url: &str,
) -> Result<()> {
    let mut current_url = initial_url.to_string();
    let mut startup = StartupScreenState {
        status: "db migrations... teaching SQLite to keep a beat".to_string(),
        logs: startup_logs(),
        frame_count: 0,
    };
    title_state.set("looper — booting".to_string())?;
    terminal.draw(|frame| draw_startup(frame, &startup))?;
    let storage = Storage::open_and_migrate()?.shared();

    loop {
        startup.frame_count += 1;
        startup.status = format!("loading song... bribing the aux cord for `{current_url}`");
        title_state.set("looper — loading song".to_string())?;
        terminal.draw(|frame| draw_startup(frame, &startup))?;

        let next = match plugin::resolve_url(&current_url)? {
            None => play_tracks(
                terminal,
                title_state,
                storage.clone(),
                None,
                vec![TrackInfo {
                    title: extract_filename(&current_url),
                    duration_secs: None,
                    playback: PlaybackInput::file(&current_url),
                    source_url: None,
                    pending_download: None,
                    service: None,
                }],
                false,
            )?,
            Some(tracks) => {
                let is_playlist = tracks.len() > 1;
                play_tracks(
                    terminal,
                    title_state,
                    storage.clone(),
                    Some(current_url.as_str()),
                    tracks,
                    is_playlist,
                )?
            }
        };

        match next {
            Some(replay_target) => current_url = replay_target,
            None => return Ok(()),
        }
    }
}

enum LoopAction {
    Quit,
    NextTrack,
    ReplayTarget(String),
}

struct TitleState {
    last_title: Option<String>,
}

impl TitleState {
    fn new() -> Self {
        Self { last_title: None }
    }

    fn set(&mut self, title: String) -> Result<()> {
        if self.last_title.as_deref() == Some(title.as_str()) {
            return Ok(());
        }

        write_terminal_title(&title)?;
        self.last_title = Some(title);
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        self.set("looper".to_string())
    }
}

fn run_loop(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    state: &mut AppState,
    player: &AudioPlayer,
    track: &TrackInfo,
    storage: SharedStorage,
    title_state: &mut TitleState,
) -> Result<LoopAction> {
    loop {
        if !state.paused {
            update_visualizer(state, player);
        }

        state.frame_count += 1;
        title_state.set(format_playback_title(
            state.frame_count,
            &state.filename,
            state.paused,
        ))?;
        terminal.draw(|f| draw(f, state))?;

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                match handle_key_event(key, state) {
                    KeyCommand::Quit => return Ok(LoopAction::Quit),
                    KeyCommand::TogglePause => {
                        if state.paused {
                            state.loop_start = Instant::now() - state.pause_elapsed;
                            player.resume();
                        } else {
                            state.pause_elapsed = state.loop_start.elapsed();
                            player.pause();
                        }
                        state.paused = !state.paused;
                    }
                    KeyCommand::ToggleFullscreen => {
                        state.fullscreen = !state.fullscreen;
                    }
                    KeyCommand::ToggleFavorite => {
                        if let Ok(record) = track_record(track) {
                            let favorite =
                                storage.lock().unwrap().toggle_favorite(&record.track_key)?;
                            state.is_favorite = favorite;
                            if let Some(panel) = state.history_panel.as_mut() {
                                refresh_history_panel(panel, &storage)?;
                            }
                        }
                    }
                    KeyCommand::ToggleHistory => {
                        toggle_history_panel(state, &storage)?;
                    }
                    KeyCommand::HistoryNext => {
                        if let Some(panel) = state.history_panel.as_mut() {
                            if panel.selected + 1 < panel.rows.len() {
                                panel.selected += 1;
                            }
                        }
                    }
                    KeyCommand::HistoryPrev => {
                        if let Some(panel) = state.history_panel.as_mut() {
                            panel.selected = panel.selected.saturating_sub(1);
                        }
                    }
                    KeyCommand::HistorySortNext => {
                        if let Some(panel) = state.history_panel.as_mut() {
                            panel.sort_field = panel.sort_field.next();
                            refresh_history_panel(panel, &storage)?;
                        }
                    }
                    KeyCommand::HistorySortPrev => {
                        if let Some(panel) = state.history_panel.as_mut() {
                            panel.sort_field = panel.sort_field.previous();
                            refresh_history_panel(panel, &storage)?;
                        }
                    }
                    KeyCommand::HistoryReverse => {
                        if let Some(panel) = state.history_panel.as_mut() {
                            panel.descending = !panel.descending;
                            refresh_history_panel(panel, &storage)?;
                        }
                    }
                    KeyCommand::HistoryReplay => {
                        if let Some(panel) = &state.history_panel {
                            if let Some(row) = panel.rows.get(panel.selected) {
                                return Ok(LoopAction::ReplayTarget(row.replay_target.clone()));
                            }
                        }
                    }
                    KeyCommand::HistoryToggleFavorite => {
                        if let Some(panel) = state.history_panel.as_mut() {
                            if let Some(row) = panel.rows.get(panel.selected) {
                                let selected_key = row.track_key.clone();
                                storage.lock().unwrap().toggle_favorite(&selected_key)?;
                                refresh_history_panel(panel, &storage)?;
                                if let Ok(record) = track_record(track) {
                                    if record.track_key == selected_key {
                                        state.is_favorite = storage
                                            .lock()
                                            .unwrap()
                                            .favorite_for(&record.track_key)?;
                                    }
                                }
                            }
                        }
                    }
                    KeyCommand::None => {}
                }
            }
        }

        if !state.paused {
            if state.is_playlist {
                if player.sink.empty() {
                    return Ok(LoopAction::NextTrack);
                }
            } else if let Some(dur) = state.duration {
                if state.elapsed() >= dur {
                    state.loop_count += 1;
                    state.loop_start = Instant::now();
                }
            }
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
enum KeyCommand {
    None,
    Quit,
    TogglePause,
    ToggleFullscreen,
    ToggleFavorite,
    ToggleHistory,
    HistoryNext,
    HistoryPrev,
    HistorySortNext,
    HistorySortPrev,
    HistoryReverse,
    HistoryReplay,
    HistoryToggleFavorite,
}

fn handle_key_event(key: KeyEvent, state: &AppState) -> KeyCommand {
    if state.history_panel.is_some() {
        match (key.code, key.modifiers) {
            (KeyCode::Esc, _) | (KeyCode::Char('p'), _) => KeyCommand::ToggleHistory,
            (KeyCode::Char('j'), _) => KeyCommand::HistoryNext,
            (KeyCode::Char('k'), _) => KeyCommand::HistoryPrev,
            (KeyCode::Char('l'), _) => KeyCommand::HistorySortNext,
            (KeyCode::Char('h'), _) => KeyCommand::HistorySortPrev,
            (KeyCode::Char('r'), _) => KeyCommand::HistoryReverse,
            (KeyCode::Char('s'), _) => KeyCommand::HistoryToggleFavorite,
            (KeyCode::Enter, _) => KeyCommand::HistoryReplay,
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                KeyCommand::Quit
            }
            _ => KeyCommand::None,
        }
    } else {
        match (key.code, key.modifiers) {
            (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                KeyCommand::Quit
            }
            (KeyCode::Char(' '), _) => KeyCommand::TogglePause,
            (KeyCode::Char('f'), _) => KeyCommand::ToggleFullscreen,
            (KeyCode::Char('s'), _) => KeyCommand::ToggleFavorite,
            (KeyCode::Char('p'), modifiers) if modifiers.contains(KeyModifiers::SUPER) => {
                KeyCommand::ToggleHistory
            }
            (KeyCode::Char('p'), _) => KeyCommand::ToggleHistory,
            _ => KeyCommand::None,
        }
    }
}

fn toggle_history_panel(state: &mut AppState, storage: &SharedStorage) -> Result<()> {
    if state.history_panel.is_some() {
        state.history_panel = None;
        return Ok(());
    }

    let mut panel = HistoryPanelState {
        rows: Vec::new(),
        selected: 0,
        sort_field: HistorySortField::LastPlayed,
        descending: true,
    };
    refresh_history_panel(&mut panel, storage)?;
    state.history_panel = Some(panel);
    Ok(())
}

fn refresh_history_panel(panel: &mut HistoryPanelState, storage: &SharedStorage) -> Result<()> {
    panel.rows = storage
        .lock()
        .unwrap()
        .list_history(panel.sort_field, panel.descending)?;
    if panel.rows.is_empty() {
        panel.selected = 0;
    } else {
        panel.selected = panel.selected.min(panel.rows.len() - 1);
    }
    Ok(())
}

/// Reads the latest samples from the audio tap, runs FFT via spectrum-analyzer,
/// maps to N_BANDS logarithmically-spaced frequency bands, and applies
/// asymmetric smoothing (fast attack, slow decay) for visual stability.
fn update_visualizer(state: &mut AppState, player: &AudioPlayer) {
    use spectrum_analyzer::windows::hann_window;
    use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};

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
        raw.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect()
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

        // Use max of bins — more sensitive than mean for sparse low-freq bands
        let raw_mag = if vals.is_empty() {
            0.0
        } else {
            vals.iter().cloned().fold(0.0_f32, f32::max)
        };

        // Per-band AGC: track each band's historical peak with a slow decay.
        // Normalizing against it ensures every band uses its full visual range.
        state.band_peak[i] = (state.band_peak[i] * 0.998).max(raw_mag).max(0.02);
        let normalized = raw_mag / state.band_peak[i];

        // Asymmetric smoothing: fast attack, faster decay than before for snappier response
        state.bands[i] = if normalized > state.prev_bands[i] {
            0.6 * normalized + 0.4 * state.prev_bands[i]
        } else {
            0.35 * normalized + 0.65 * state.prev_bands[i]
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

fn play_tracks(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    storage: SharedStorage,
    source_url: Option<&str>,
    tracks: Vec<TrackInfo>,
    is_playlist: bool,
) -> Result<Option<String>> {
    if is_playlist {
        loop_playlist(terminal, source_url, tracks, storage, title_state)
    } else {
        match play_single_track(
            terminal,
            tracks[0].clone(),
            1,
            1,
            false,
            storage,
            title_state,
        )? {
            LoopAction::Quit => Ok(None),
            LoopAction::NextTrack => Ok(None),
            LoopAction::ReplayTarget(target) => Ok(Some(target)),
        }
    }
}

fn loop_playlist(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    source_url: Option<&str>,
    initial_tracks: Vec<TrackInfo>,
    storage: SharedStorage,
    title_state: &mut TitleState,
) -> Result<Option<String>> {
    let mut tracks = initial_tracks;
    loop {
        let shared_tracks = Arc::new(Mutex::new(tracks));
        let prefetch_worker = PrefetchWorker::spawn(Arc::clone(&shared_tracks));
        let mut prefetched = HashSet::new();
        let total_tracks = shared_tracks.lock().unwrap().len();

        for idx in 0..total_tracks {
            prefetch_worker.enqueue(idx, &mut prefetched);
            prefetch_worker.enqueue(idx + 1, &mut prefetched);

            let track = {
                let tracks = shared_tracks.lock().unwrap();
                tracks[idx].clone()
            };

            match play_single_track(
                terminal,
                track,
                idx + 1,
                total_tracks,
                true,
                storage.clone(),
                title_state,
            )? {
                LoopAction::Quit => return Ok(None),
                LoopAction::NextTrack => continue,
                LoopAction::ReplayTarget(target) => return Ok(Some(target)),
            }
        }

        if let Some(url) = source_url {
            tracks =
                plugin::resolve_url(url)?.unwrap_or_else(|| shared_tracks.lock().unwrap().clone());
        } else {
            tracks = shared_tracks.lock().unwrap().clone();
        }
    }
}

fn play_single_track(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    mut track: TrackInfo,
    track_index: usize,
    total_tracks: usize,
    is_playlist: bool,
    storage: SharedStorage,
    title_state: &mut TitleState,
) -> Result<LoopAction> {
    if prepare_track_for_playback(
        terminal,
        &mut track,
        track_index,
        total_tracks,
        is_playlist,
        title_state,
    )? {
        return Ok(LoopAction::Quit);
    }

    let player = AudioPlayer::new(track.playback.clone(), !is_playlist)?;
    let record = track_record(&track)?;
    {
        let storage = storage.lock().unwrap();
        storage.record_play(&record)?;
    }
    let is_favorite = storage.lock().unwrap().favorite_for(&record.track_key)?;

    let mut state = AppState {
        filename: track.title.clone(),
        service: track.service.clone(),
        is_favorite,
        duration: player
            .duration
            .or_else(|| track.duration_secs.map(Duration::from_secs_f64)),
        paused: false,
        loop_count: 1,
        track_index,
        total_tracks,
        is_playlist,
        loop_start: Instant::now(),
        pause_elapsed: Duration::default(),
        bands: vec![0.0; N_BANDS],
        prev_bands: vec![0.0; N_BANDS],
        band_peak: vec![0.02; N_BANDS],
        fullscreen: false,
        frame_count: 0,
        cache_status: None,
        history_panel: None,
    };

    run_loop(terminal, &mut state, &player, &track, storage, title_state)
}

struct PrefetchWorker {
    tracks: Arc<Mutex<Vec<TrackInfo>>>,
    sender: SyncSender<PrefetchTask>,
}

#[derive(Clone)]
struct PrefetchTask {
    idx: usize,
    title: String,
    source_url: String,
}

impl PrefetchWorker {
    fn spawn(tracks: Arc<Mutex<Vec<TrackInfo>>>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(2);
        thread::spawn({
            let tracks = Arc::clone(&tracks);
            move || prefetch_worker_loop(tracks, receiver)
        });
        Self { tracks, sender }
    }

    fn enqueue(&self, idx: usize, prefetched: &mut HashSet<usize>) {
        if prefetched.contains(&idx) {
            return;
        }

        let task = {
            let tracks = self.tracks.lock().unwrap();
            if idx >= tracks.len() {
                return;
            }
            let track = tracks[idx].clone();
            if track.pending_download.is_none() && matches!(track.playback, PlaybackInput::File(_))
            {
                return;
            }
            let Some(source_url) = track
                .pending_download
                .as_ref()
                .map(|pending| pending.source_url.clone())
                .or(track.source_url.clone())
            else {
                return;
            };
            PrefetchTask {
                idx,
                title: track.title,
                source_url,
            }
        };

        match self.sender.try_send(task) {
            Ok(()) => {
                prefetched.insert(idx);
            }
            Err(TrySendError::Full(task)) => {
                eprintln!("Prefetch queue is full; skipping '{}' for now.", task.title);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

fn prefetch_worker_loop(tracks: Arc<Mutex<Vec<TrackInfo>>>, receiver: Receiver<PrefetchTask>) {
    let Ok(cache_dir) = plugin::cache_dir_path() else {
        eprintln!("Prefetch disabled: failed to resolve looper cache directory.");
        return;
    };

    while let Ok(task) = receiver.recv() {
        eprintln!("Prefetching: {}...", task.title);
        match ytdlp::download_track(&task.source_url, &cache_dir) {
            Ok(local_path) => {
                if let Ok(mut tracks) = tracks.lock() {
                    if let Some(track) = tracks.get_mut(task.idx) {
                        track.playback = PlaybackInput::file(local_path);
                        track.pending_download = None;
                        eprintln!("Cached: {}", track.title);
                    }
                }
            }
            Err(err) => {
                eprintln!("Prefetch failed for '{}': {}", task.title, err);
            }
        }
    }
}

fn prepare_track_for_playback(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    track: &mut TrackInfo,
    track_index: usize,
    total_tracks: usize,
    is_playlist: bool,
    title_state: &mut TitleState,
) -> Result<bool> {
    if let PlaybackInput::File(path) = &track.playback {
        if path.exists() {
            track.pending_download = None;
            return Ok(false);
        }
    }

    let Some(pending) = track.pending_download.clone() else {
        return Ok(false);
    };
    let cache_dir = plugin::cache_dir_path()?;
    let (sender, receiver) = mpsc::channel();
    let source_url = pending.source_url.clone();
    thread::spawn(move || {
        if let Err(err) =
            ytdlp::download_track_with_progress(&source_url, &cache_dir, Some(sender.clone()))
        {
            let _ = sender.send(DownloadEvent::Error(err.to_string()));
        }
    });

    let mut loading = LoadingState::new(
        track.title.clone(),
        pending.service,
        track_index,
        total_tracks,
        is_playlist,
    );

    loop {
        loading.frame_count += 1;
        title_state.set(format_loading_title(loading.frame_count, &loading.title))?;
        terminal.draw(|frame| draw_loading(frame, &loading))?;

        while let Ok(event) = receiver.try_recv() {
            match event {
                DownloadEvent::Progress(progress) => {
                    loading.progress = progress;
                    loading.phase = LoadingPhase::Downloading;
                }
                DownloadEvent::Finalizing => {
                    loading.phase = LoadingPhase::Finalizing;
                }
                DownloadEvent::Ready(path) => {
                    loading.phase = LoadingPhase::Ready;
                    track.playback = PlaybackInput::file(path);
                    track.pending_download = None;
                    return Ok(false);
                }
                DownloadEvent::Error(message) => {
                    loading.phase = LoadingPhase::Error(message.clone());
                    terminal.draw(|frame| draw_loading(frame, &loading))?;
                    thread::sleep(Duration::from_millis(900));
                    return Err(color_eyre::eyre::eyre!(message));
                }
            }
        }

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                match (key.code, key.modifiers) {
                    (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                        return Ok(true);
                    }
                    _ => {}
                }
            }
        }
    }
}

fn format_playback_title(frame_count: u64, title: &str, paused: bool) -> String {
    let title = truncate_title(title, 48);
    if paused {
        format!("⏸ looper — {title}")
    } else {
        format!("{} looper — {title}", spinner_frame(frame_count))
    }
}

fn format_loading_title(frame_count: u64, title: &str) -> String {
    let title = truncate_title(title, 40);
    format!("{} looper — loading — {title}", spinner_frame(frame_count))
}

fn truncate_title(title: &str, max_chars: usize) -> String {
    let mut chars = title.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

fn spinner_frame(frame_count: u64) -> char {
    const FRAMES: [char; 4] = ['◐', '◓', '◑', '◒'];
    FRAMES[((frame_count / 6) as usize) % FRAMES.len()]
}

fn write_terminal_title(title: &str) -> Result<()> {
    let mut out = stdout();
    write!(out, "\x1b]0;{}\x07", title)?;
    out.flush()?;
    Ok(())
}

fn startup_logs() -> Vec<String> {
    vec![
        "warming up the loop engine".to_string(),
        "convincing sqlite this is definitely a music venue".to_string(),
        "dusting fingerprints off the play count ledger".to_string(),
        "aligning vibes, bits, and questionable dance moves".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::HistoryRow;

    fn base_state() -> AppState {
        AppState {
            filename: "song".into(),
            service: Some("YouTube".into()),
            is_favorite: false,
            duration: None,
            paused: false,
            loop_count: 1,
            track_index: 1,
            total_tracks: 1,
            is_playlist: false,
            loop_start: Instant::now(),
            pause_elapsed: Duration::default(),
            bands: vec![0.0; N_BANDS],
            prev_bands: vec![0.0; N_BANDS],
            band_peak: vec![0.02; N_BANDS],
            fullscreen: false,
            frame_count: 0,
            cache_status: None,
            history_panel: None,
        }
    }

    #[test]
    fn plain_p_opens_history() {
        let state = base_state();
        assert_eq!(
            handle_key_event(
                KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE),
                &state
            ),
            KeyCommand::ToggleHistory
        );
    }

    #[test]
    fn history_uses_vim_keys() {
        let mut state = base_state();
        state.history_panel = Some(HistoryPanelState {
            rows: vec![HistoryRow {
                track_key: "a".into(),
                replay_target: "a".into(),
                title: "A".into(),
                platform: "Local".into(),
                is_favorite: false,
                play_count: 1,
                first_played_at: 0,
                last_played_at: 0,
            }],
            selected: 0,
            sort_field: HistorySortField::LastPlayed,
            descending: true,
        });

        assert_eq!(
            handle_key_event(
                KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE),
                &state
            ),
            KeyCommand::HistoryNext
        );
        assert_eq!(
            handle_key_event(
                KeyEvent::new(KeyCode::Char('k'), KeyModifiers::NONE),
                &state
            ),
            KeyCommand::HistoryPrev
        );
        assert_eq!(
            handle_key_event(
                KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE),
                &state
            ),
            KeyCommand::HistorySortPrev
        );
        assert_eq!(
            handle_key_event(
                KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE),
                &state
            ),
            KeyCommand::HistorySortNext
        );
    }
}
