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

use ratatui_image::picker::Picker;

use crate::audio::AudioPlayer;
use crate::download::{DownloadEvent, LoadingPhase};
use crate::media_controls::MediaSessionHandle;
use crate::playback_input::PlaybackInput;
use crate::plugin::{self, ytdlp, TrackInfo};
use crate::storage::{track_record, HistorySortField, SharedStorage, Storage, SyncWarning};
use crate::tui::{
    draw, draw_history_browser, draw_startup, restore_terminal, setup_terminal, AppState,
    HistoryPanelState, StartupScreenState, N_BANDS,
};

pub struct PlaybackContext<'a> {
    pub cmd_rx: &'a Receiver<KeyCommand>,
    pub media: Option<MediaSessionHandle>,
}

pub fn browse_history(ctx: PlaybackContext) -> Result<()> {
    run_terminal_session(|terminal, title_state, picker| {
        browse_history_session(terminal, title_state, &ctx, picker)
    })
}

pub fn play_file(url: &str, ctx: PlaybackContext) -> Result<()> {
    run_terminal_session(|terminal, title_state, picker| {
        play_file_session(terminal, title_state, url, &ctx, picker)
    })
}

fn run_terminal_session<F>(session: F) -> Result<()>
where
    F: FnOnce(
        &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
        &mut TitleState,
        Option<&Picker>,
    ) -> Result<()>,
{
    #[cfg(unix)]
    reattach_stdin_to_tty()?;

    let (mut terminal, picker) = setup_terminal()?;
    let mut title_state = TitleState::new();

    // Wrap color_eyre's panic hook so the terminal is restored before the
    // panic message is printed.
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen);
        orig_hook(info);
    }));

    let result = session(&mut terminal, &mut title_state, picker.as_ref());

    let _ = title_state.reset();
    restore_terminal(&mut terminal)?;
    result
}

fn browse_history_session(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    ctx: &PlaybackContext,
    picker: Option<&Picker>,
) -> Result<()> {
    let mut startup = StartupScreenState {
        status: "db migrations... teaching SQLite to keep a beat".to_string(),
        logs: startup_logs(),
        frame_count: 0,
        sync_warning: None,
    };
    title_state.set("looper — playlist history".to_string())?;
    terminal.draw(|frame| draw_startup(frame, &startup))?;

    let (storage, sync_warning) = Storage::open_and_migrate()?;
    let storage = storage.shared();
    startup.sync_warning = sync_warning.clone();
    let mut panel = HistoryPanelState {
        rows: Vec::new(),
        selected: 0,
        sort_field: HistorySortField::TimePlayed,
        descending: true,
    };
    refresh_history_panel(&mut panel, &storage)?;

    loop {
        startup.frame_count += 1;
        title_state.set("looper — playlist history".to_string())?;
        terminal.draw(|frame| draw_history_browser(frame, &panel, sync_warning.as_ref()))?;

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                match handle_history_browser_key_event(key, &panel) {
                    KeyCommand::Quit => return Ok(()),
                    KeyCommand::HistoryNext => {
                        if panel.selected + 1 < panel.rows.len() {
                            panel.selected += 1;
                        }
                    }
                    KeyCommand::HistoryPrev => {
                        panel.selected = panel.selected.saturating_sub(1);
                    }
                    KeyCommand::HistorySortNext => {
                        panel.sort_field = panel.sort_field.next();
                        refresh_history_panel(&mut panel, &storage)?;
                    }
                    KeyCommand::HistorySortPrev => {
                        panel.sort_field = panel.sort_field.previous();
                        refresh_history_panel(&mut panel, &storage)?;
                    }
                    KeyCommand::HistoryReverse => {
                        panel.descending = !panel.descending;
                        refresh_history_panel(&mut panel, &storage)?;
                    }
                    KeyCommand::HistoryToggleFavorite => {
                        if let Some(row) = panel.rows.get(panel.selected) {
                            storage.lock().unwrap().toggle_favorite(&row.track_key)?;
                            refresh_history_panel(&mut panel, &storage)?;
                        }
                    }
                    KeyCommand::HistoryReplay => {
                        if let Some(row) = panel.rows.get(panel.selected) {
                            return play_file_session(
                                terminal,
                                title_state,
                                &row.replay_target,
                                ctx,
                                picker,
                            );
                        }
                    }
                    KeyCommand::None
                    | KeyCommand::TogglePause
                    | KeyCommand::NextTrack
                    | KeyCommand::PreviousTrack
                    | KeyCommand::ToggleFullscreen
                    | KeyCommand::ToggleFavorite
                    | KeyCommand::ToggleHistory => {}
                }
            }
        }
    }
}

fn play_file_session(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    initial_url: &str,
    ctx: &PlaybackContext,
    picker: Option<&Picker>,
) -> Result<()> {
    let mut current_url = initial_url.to_string();
    let mut startup = StartupScreenState {
        status: "db migrations... teaching SQLite to keep a beat".to_string(),
        logs: startup_logs(),
        frame_count: 0,
        sync_warning: None,
    };
    title_state.set("looper — booting".to_string())?;
    terminal.draw(|frame| draw_startup(frame, &startup))?;
    let (storage, sync_warning) = Storage::open_and_migrate()?;
    let storage = storage.shared();
    startup.sync_warning = sync_warning.clone();

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
                    thumbnail_path: None,
                }],
                false,
                ctx,
                sync_warning.as_ref(),
                picker,
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
                    ctx,
                    sync_warning.as_ref(),
                    picker,
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
    PreviousTrack,
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
    ctx: &PlaybackContext,
) -> Result<LoopAction> {
    // Frame rate control: only render when needed
    const RENDER_FPS: u64 = 30; // Target 30 FPS max
    let render_interval = Duration::from_millis(1000 / RENDER_FPS);
    let mut last_render = Instant::now();
    let mut needs_render: bool;

    loop {
        // Update visualizer data (but don't render yet)
        if !state.paused {
            update_visualizer(state, player);
        }
        // Always render when playing or paused (wave screensaver needs frame_count to advance)
        needs_render = true;

        // Check if enough time passed for next frame
        let time_since_render = last_render.elapsed();
        if time_since_render < render_interval {
            // Too soon to render, check for events with shorter timeout
            needs_render = false;
        }

        if event::poll(Duration::from_millis(30))? {
            if let Event::Key(key) = event::read()? {
                let cmd = handle_key_event(key, state);
                if let Some(action) = dispatch_command(
                    cmd,
                    state,
                    player,
                    track,
                    &storage,
                    &mut needs_render,
                    ctx.media.as_ref(),
                )? {
                    return Ok(action);
                }
            }
        }

        while let Ok(cmd) = ctx.cmd_rx.try_recv() {
            if let Some(action) = dispatch_command(
                cmd,
                state,
                player,
                track,
                &storage,
                &mut needs_render,
                ctx.media.as_ref(),
            )? {
                return Ok(action);
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

        // Only render when needed (state changed or enough time passed)
        if needs_render && last_render.elapsed() >= render_interval {
            state.frame_count += 1;
            title_state.set(format_playback_title(
                state.frame_count,
                &state.filename,
                state.paused,
            ))?;
            terminal.draw(|f| draw(f, state))?;
            last_render = Instant::now();
        }
    }
}

fn dispatch_command(
    cmd: KeyCommand,
    state: &mut AppState,
    player: &AudioPlayer,
    track: &TrackInfo,
    storage: &SharedStorage,
    needs_render: &mut bool,
    media: Option<&MediaSessionHandle>,
) -> Result<Option<LoopAction>> {
    match cmd {
        KeyCommand::Quit => Ok(Some(LoopAction::Quit)),
        KeyCommand::TogglePause => {
            if state.paused {
                state.loop_start = Instant::now() - state.pause_elapsed;
                player.resume();
            } else {
                state.pause_elapsed = state.loop_start.elapsed();
                player.pause();
            }
            state.paused = !state.paused;
            if let Some(media) = media {
                media.set_playback(state.paused, state.elapsed());
            }
            *needs_render = true;
            Ok(None)
        }
        KeyCommand::NextTrack => {
            if state.is_playlist {
                player.skip();
                Ok(Some(LoopAction::NextTrack))
            } else {
                Ok(None)
            }
        }
        KeyCommand::PreviousTrack => {
            if state.is_playlist {
                player.skip();
                Ok(Some(LoopAction::PreviousTrack))
            } else {
                Ok(None)
            }
        }
        KeyCommand::ToggleFullscreen => {
            state.fullscreen = !state.fullscreen;
            *needs_render = true;
            Ok(None)
        }
        KeyCommand::ToggleFavorite => {
            if let Ok(record) = track_record(track) {
                let favorite = storage.lock().unwrap().toggle_favorite(&record.track_key)?;
                state.is_favorite = favorite;
                if let Some(panel) = state.history_panel.as_mut() {
                    refresh_history_panel(panel, storage)?;
                }
            }
            *needs_render = true;
            Ok(None)
        }
        KeyCommand::ToggleHistory => {
            toggle_history_panel(state, storage)?;
            *needs_render = true;
            Ok(None)
        }
        KeyCommand::HistoryNext => {
            if let Some(panel) = state.history_panel.as_mut() {
                if panel.selected + 1 < panel.rows.len() {
                    panel.selected += 1;
                    *needs_render = true;
                }
            }
            Ok(None)
        }
        KeyCommand::HistoryPrev => {
            if let Some(panel) = state.history_panel.as_mut() {
                panel.selected = panel.selected.saturating_sub(1);
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::HistorySortNext => {
            if let Some(panel) = state.history_panel.as_mut() {
                panel.sort_field = panel.sort_field.next();
                refresh_history_panel(panel, storage)?;
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::HistorySortPrev => {
            if let Some(panel) = state.history_panel.as_mut() {
                panel.sort_field = panel.sort_field.previous();
                refresh_history_panel(panel, storage)?;
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::HistoryReverse => {
            if let Some(panel) = state.history_panel.as_mut() {
                panel.descending = !panel.descending;
                refresh_history_panel(panel, storage)?;
                *needs_render = true;
            }
            Ok(None)
        }
        KeyCommand::HistoryReplay => {
            if let Some(panel) = &state.history_panel {
                if let Some(row) = panel.rows.get(panel.selected) {
                    return Ok(Some(LoopAction::ReplayTarget(row.replay_target.clone())));
                }
            }
            Ok(None)
        }
        KeyCommand::HistoryToggleFavorite => {
            if let Some(panel) = state.history_panel.as_mut() {
                if let Some(row) = panel.rows.get(panel.selected) {
                    let selected_key = row.track_key.clone();
                    storage.lock().unwrap().toggle_favorite(&selected_key)?;
                    refresh_history_panel(panel, storage)?;
                    if let Ok(record) = track_record(track) {
                        if record.track_key == selected_key {
                            state.is_favorite =
                                storage.lock().unwrap().favorite_for(&record.track_key)?;
                        }
                    }
                }
            }
            *needs_render = true;
            Ok(None)
        }
        KeyCommand::None => Ok(None),
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum KeyCommand {
    None,
    Quit,
    TogglePause,
    NextTrack,
    PreviousTrack,
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
            (KeyCode::Char('n'), _) => KeyCommand::NextTrack,
            (KeyCode::Char('b'), _) => KeyCommand::PreviousTrack,
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

fn handle_history_browser_key_event(key: KeyEvent, panel: &HistoryPanelState) -> KeyCommand {
    match (key.code, key.modifiers) {
        (KeyCode::Char('j'), _) => KeyCommand::HistoryNext,
        (KeyCode::Char('k'), _) => KeyCommand::HistoryPrev,
        (KeyCode::Char('l'), _) => KeyCommand::HistorySortNext,
        (KeyCode::Char('h'), _) => KeyCommand::HistorySortPrev,
        (KeyCode::Char('r'), _) => KeyCommand::HistoryReverse,
        (KeyCode::Char('s'), _) if !panel.rows.is_empty() => KeyCommand::HistoryToggleFavorite,
        (KeyCode::Enter, _) if !panel.rows.is_empty() => KeyCommand::HistoryReplay,
        (KeyCode::Char('q'), _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => KeyCommand::Quit,
        _ => KeyCommand::None,
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
        sort_field: HistorySortField::TimePlayed,
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

fn persist_played_time(
    storage: &SharedStorage,
    track_key: &str,
    played_seconds: i64,
) -> Result<()> {
    storage
        .lock()
        .unwrap()
        .record_playback_time(track_key, played_seconds)?;
    Ok(())
}

fn played_seconds(state: &AppState) -> i64 {
    if state.is_playlist {
        return state.elapsed().as_secs() as i64;
    }

    match state.duration {
        Some(duration) if duration.as_secs() > 0 => {
            let completed_loops = state.loop_count.saturating_sub(1);
            (completed_loops * duration.as_secs() + state.elapsed().as_secs()) as i64
        }
        _ => state.elapsed().as_secs() as i64,
    }
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
    ctx: &PlaybackContext,
    sync_warning: Option<&SyncWarning>,
    picker: Option<&Picker>,
) -> Result<Option<String>> {
    if is_playlist {
        loop_playlist(
            terminal,
            source_url,
            tracks,
            storage,
            title_state,
            ctx,
            sync_warning,
            picker,
        )
    } else {
        match play_single_track(
            terminal,
            tracks[0].clone(),
            1,
            1,
            false,
            storage,
            title_state,
            ctx,
            sync_warning,
            picker,
        )? {
            LoopAction::Quit => Ok(None),
            LoopAction::NextTrack | LoopAction::PreviousTrack => Ok(None),
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
    ctx: &PlaybackContext,
    sync_warning: Option<&SyncWarning>,
    picker: Option<&Picker>,
) -> Result<Option<String>> {
    let mut tracks = initial_tracks;
    loop {
        let shared_tracks = Arc::new(Mutex::new(tracks));
        let prefetch_worker = PrefetchWorker::spawn(Arc::clone(&shared_tracks));
        let mut prefetched = HashSet::new();
        let total_tracks = shared_tracks.lock().unwrap().len();

        let mut idx: usize = 0;
        while idx < total_tracks {
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
                ctx,
                sync_warning,
                picker,
            )? {
                LoopAction::Quit => return Ok(None),
                LoopAction::NextTrack => idx += 1,
                LoopAction::PreviousTrack => idx = idx.saturating_sub(1),
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

/// Open and decode a thumbnail image into a ratatui-image stateful protocol.
/// Returns None if any step fails — the renderer treats that as "no image."
fn decode_thumbnail(
    picker: Option<&Picker>,
    path: Option<&Path>,
) -> Option<ratatui_image::protocol::StatefulProtocol> {
    let picker = picker?;
    let path = path?;
    let dyn_img = image::ImageReader::open(path).ok()?.decode().ok()?;
    Some(picker.new_resize_protocol(dyn_img))
}

fn play_single_track(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    mut track: TrackInfo,
    track_index: usize,
    total_tracks: usize,
    is_playlist: bool,
    storage: SharedStorage,
    title_state: &mut TitleState,
    ctx: &PlaybackContext,
    sync_warning: Option<&SyncWarning>,
    picker: Option<&Picker>,
) -> Result<LoopAction> {
    if prepare_track_for_playback(
        terminal,
        &mut track,
        track_index,
        total_tracks,
        is_playlist,
        title_state,
        sync_warning,
    )? {
        return Ok(LoopAction::Quit);
    }

    render_track_startup(
        terminal,
        title_state,
        &track,
        track_index,
        total_tracks,
        is_playlist,
        LoadingPhase::Finalizing,
        None,
        "patching cables into the tiny disco".to_string(),
        0,
        sync_warning,
    )?;
    let player = AudioPlayer::new(track.playback.clone(), !is_playlist)?;
    wait_for_player_ready(
        terminal,
        title_state,
        &track,
        track_index,
        total_tracks,
        is_playlist,
        &player,
        sync_warning,
    )?;
    let record = track_record(&track)?;
    {
        let storage = storage.lock().unwrap();
        storage.record_play(&record)?;
    }
    let is_favorite = storage.lock().unwrap().favorite_for(&record.track_key)?;

    let thumbnail = decode_thumbnail(picker, track.thumbnail_path.as_deref());

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
        sync_warning: sync_warning.cloned(),
        thumbnail,
    };

    if let Some(media) = &ctx.media {
        media.set_metadata(&track);
        media.set_playback(false, Duration::default());
    }

    let result = run_loop(
        terminal,
        &mut state,
        &player,
        &track,
        storage.clone(),
        title_state,
        ctx,
    )?;
    persist_played_time(&storage, &record.track_key, played_seconds(&state))?;
    Ok(result)
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
    sync_warning: Option<&SyncWarning>,
) -> Result<bool> {
    if let PlaybackInput::File(path) = &track.playback {
        if path.exists() {
            // Audio is already cached. Backfill the thumbnail if it's missing
            // and we have a remote source URL — covers the case where the mp3
            // was downloaded before --write-thumbnail was added to the audio
            // download command.
            if track.thumbnail_path.is_none() {
                if let (Some(stem), Some(source_url)) = (
                    path.file_stem().and_then(|s| s.to_str()).map(str::to_string),
                    track.source_url.clone(),
                ) {
                    if let Ok(cache_dir) = plugin::cache_dir_path() {
                        let _ = ytdlp::download_thumbnail_only(&source_url, &cache_dir);
                        track.thumbnail_path = ytdlp::thumbnail_for(&cache_dir, &stem);
                    }
                }
            }
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
    let cache_dir_for_dl = cache_dir.clone();
    thread::spawn(move || {
        if let Err(err) = ytdlp::download_track_with_progress(
            &source_url,
            &cache_dir_for_dl,
            Some(sender.clone()),
        ) {
            let _ = sender.send(DownloadEvent::Error(err.to_string()));
        }
    });

    let mut frame_count = 0_u64;
    let mut phase = LoadingPhase::Resolving;
    let mut progress = None;
    let mut note = "reading the tea leaves in remote metadata".to_string();

    loop {
        frame_count += 1;
        render_track_startup(
            terminal,
            title_state,
            track,
            track_index,
            total_tracks,
            is_playlist,
            phase.clone(),
            progress.clone(),
            note.clone(),
            frame_count,
            sync_warning,
        )?;

        while let Ok(event) = receiver.try_recv() {
            match event {
                DownloadEvent::Progress(next_progress) => {
                    progress = Some(next_progress);
                    phase = LoadingPhase::Downloading;
                    note = "teaching bytes to moonwalk into the cache".to_string();
                }
                DownloadEvent::Finalizing => {
                    phase = LoadingPhase::Finalizing;
                    note = "teaching ffmpeg some manners before showtime".to_string();
                }
                DownloadEvent::Ready(path) => {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        track.thumbnail_path = ytdlp::thumbnail_for(&cache_dir, stem);
                    }
                    track.playback = PlaybackInput::file(path);
                    track.pending_download = None;
                    return Ok(false);
                }
                DownloadEvent::Error(message) => {
                    render_track_startup(
                        terminal,
                        title_state,
                        track,
                        track_index,
                        total_tracks,
                        is_playlist,
                        LoadingPhase::Error(message.clone()),
                        progress.clone(),
                        message.clone(),
                        frame_count,
                        sync_warning,
                    )?;
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

fn wait_for_player_ready(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    track: &TrackInfo,
    track_index: usize,
    total_tracks: usize,
    is_playlist: bool,
    player: &AudioPlayer,
    sync_warning: Option<&SyncWarning>,
) -> Result<()> {
    let start = Instant::now();
    let mut frame_count = 0_u64;

    loop {
        let buffered_samples = player.sample_buf.lock().unwrap().len();
        if buffered_samples > 0 || start.elapsed() >= Duration::from_millis(750) {
            return Ok(());
        }

        frame_count += 1;
        render_track_startup(
            terminal,
            title_state,
            track,
            track_index,
            total_tracks,
            is_playlist,
            LoadingPhase::Ready,
            None,
            "priming the speakers so the first hit lands clean".to_string(),
            frame_count,
            sync_warning,
        )?;
        thread::sleep(Duration::from_millis(30));
    }
}

fn render_track_startup(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    title_state: &mut TitleState,
    track: &TrackInfo,
    track_index: usize,
    total_tracks: usize,
    is_playlist: bool,
    phase: LoadingPhase,
    progress: Option<crate::download::DownloadProgress>,
    note: String,
    frame_count: u64,
    sync_warning: Option<&SyncWarning>,
) -> Result<()> {
    title_state.set(format_loading_title(frame_count, &track.title))?;
    let status = startup_status(
        track,
        track_index,
        total_tracks,
        is_playlist,
        &phase,
        progress,
    );
    let startup = StartupScreenState {
        status,
        logs: track_startup_logs(note),
        frame_count,
        sync_warning: sync_warning.cloned(),
    };
    terminal.draw(|frame| draw_startup(frame, &startup))?;
    Ok(())
}

fn startup_status(
    track: &TrackInfo,
    track_index: usize,
    total_tracks: usize,
    is_playlist: bool,
    phase: &LoadingPhase,
    progress: Option<crate::download::DownloadProgress>,
) -> String {
    let position = if is_playlist {
        format!("track {track_index}/{total_tracks}")
    } else {
        "single track".to_string()
    };
    let service = track.service.as_deref().unwrap_or("Local");
    let title = truncate_title(&track.title, 48);

    match phase {
        LoadingPhase::Resolving => format!("{position} • {service} • sizing up `{title}`"),
        LoadingPhase::Downloading => {
            let progress_label = progress
                .map(|p| {
                    let percent = p
                        .fraction()
                        .map(|f| format!("{}%", (f * 100.0).round() as u64))
                        .unwrap_or_else(|| "warming up".to_string());
                    let speed = p
                        .speed_bytes_per_sec
                        .map(human_speed)
                        .unwrap_or_else(|| "--".to_string());
                    let eta = p
                        .eta_seconds
                        .map(human_eta)
                        .unwrap_or_else(|| "--:--".to_string());
                    format!("{percent} • {speed} • eta {eta}")
                })
                .unwrap_or_else(|| "warming up".to_string());
            format!("{position} • {service} • downloading `{title}` • {progress_label}")
        }
        LoadingPhase::Finalizing => format!("{position} • {service} • polishing `{title}`"),
        LoadingPhase::Ready => format!("{position} • {service} • cueing `{title}`"),
        LoadingPhase::Error(message) => {
            format!("{position} • {service} • {}", truncate_title(message, 48))
        }
    }
}

fn track_startup_logs(note: String) -> Vec<String> {
    vec![
        note,
        "checking that the beat and the database remain legally married".to_string(),
        "keeping the stage curtains closed until audio is actually ready".to_string(),
    ]
}

fn human_speed(bytes_per_sec: u64) -> String {
    format!("{}/s", human_bytes(bytes_per_sec))
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn human_eta(eta: u64) -> String {
    let minutes = eta / 60;
    let seconds = eta % 60;
    if minutes >= 60 {
        let hours = minutes / 60;
        let minutes = minutes % 60;
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
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
            sync_warning: None,
            thumbnail: None,
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
                total_play_seconds: 10,
                first_played_at: 0,
                last_played_at: 0,
                last_played_computer: String::new(),
            }],
            selected: 0,
            sort_field: HistorySortField::TimePlayed,
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

    #[test]
    fn history_browser_uses_replay_and_quit_keys() {
        let panel = HistoryPanelState {
            rows: vec![HistoryRow {
                track_key: "a".into(),
                replay_target: "a".into(),
                title: "A".into(),
                platform: "Local".into(),
                is_favorite: false,
                play_count: 1,
                total_play_seconds: 10,
                first_played_at: 0,
                last_played_at: 0,
                last_played_computer: String::new(),
            }],
            selected: 0,
            sort_field: HistorySortField::TimePlayed,
            descending: true,
        };

        assert_eq!(
            handle_history_browser_key_event(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &panel
            ),
            KeyCommand::HistoryReplay
        );
        assert_eq!(
            handle_history_browser_key_event(
                KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
                &panel
            ),
            KeyCommand::Quit
        );
    }

    #[test]
    fn history_browser_ignores_replay_when_empty() {
        let panel = HistoryPanelState {
            rows: Vec::new(),
            selected: 0,
            sort_field: HistorySortField::TimePlayed,
            descending: true,
        };

        assert_eq!(
            handle_history_browser_key_event(
                KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
                &panel
            ),
            KeyCommand::None
        );
    }
}
