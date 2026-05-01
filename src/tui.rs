use chrono::{Local, TimeZone};
use color_eyre::eyre::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState},
    Terminal,
};
use std::{
    io::{stdout, Stdout},
    time::{Duration, Instant},
};

use crate::download::CacheStatus;
use crate::storage::{HistoryRow, HistorySortField, SyncWarning};

pub const N_BANDS: usize = 32;
const PROGRESS_KNOB: char = '•';

pub struct AppState {
    pub filename: String,
    pub service: Option<String>,
    pub is_favorite: bool,
    pub duration: Option<Duration>,
    pub paused: bool,
    pub loop_count: u64,
    pub track_index: usize,
    pub total_tracks: usize,
    pub is_playlist: bool,
    pub loop_start: Instant,
    pub pause_elapsed: Duration,
    pub bands: Vec<f32>,
    pub prev_bands: Vec<f32>,
    pub band_peak: Vec<f32>,
    pub fullscreen: bool,
    pub frame_count: u64,
    pub cache_status: Option<CacheStatus>,
    pub history_panel: Option<HistoryPanelState>,
    pub sync_warning: Option<SyncWarning>,
}

#[derive(Clone)]
pub struct HistoryPanelState {
    pub rows: Vec<HistoryRow>,
    pub selected: usize,
    pub sort_field: HistorySortField,
    pub descending: bool,
}

#[derive(Clone)]
pub struct StartupScreenState {
    pub status: String,
    pub logs: Vec<String>,
    pub frame_count: u64,
    pub sync_warning: Option<SyncWarning>,
}

impl AppState {
    /// Playback-aware elapsed time: freezes when paused.
    pub fn elapsed(&self) -> Duration {
        if self.paused {
            self.pause_elapsed
        } else {
            self.loop_start.elapsed()
        }
    }
}

pub fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut out = stdout();
    execute!(out, EnterAlternateScreen)?;
    let terminal = Terminal::new(CrosstermBackend::new(out))?;
    Ok(terminal)
}

pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

/// Height (in rows) consumed by the sync-warning banner when present.
/// Bordered box: 1 top border + 5 content lines + 1 bottom border.
const SYNC_WARNING_HEIGHT: u16 = 7;

pub fn draw(frame: &mut ratatui::Frame, state: &AppState) {
    let body_area = match state.sync_warning.as_ref() {
        Some(warning) => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(SYNC_WARNING_HEIGHT),
                    Constraint::Min(0),
                ])
                .split(frame.area());
            draw_sync_warning(frame, chunks[0], warning);
            chunks[1]
        }
        None => frame.area(),
    };

    if state.fullscreen {
        draw_fullscreen_in(frame, body_area, state);
    } else {
        draw_normal_in(frame, body_area, state);
    }

    if let Some(panel) = &state.history_panel {
        draw_history_panel(frame, state, panel);
    }
}

fn draw_sync_warning(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, warning: &SyncWarning) {
    let label_style = Style::default()
        .fg(Color::Rgb(255, 200, 90))
        .add_modifier(Modifier::BOLD);
    let value_style = Style::default().fg(Color::Rgb(230, 230, 240));
    let dim_style = Style::default().fg(Color::Rgb(170, 170, 190));

    let lines = vec![
        Line::from(vec![
            Span::styled("⚠ ", label_style),
            Span::styled(
                "History sync disabled — running on local DB",
                Style::default()
                    .fg(Color::Rgb(255, 200, 90))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Path:    ", label_style),
            Span::styled(warning.attempted_path.display().to_string(), value_style),
        ]),
        Line::from(vec![
            Span::styled("  Reason:  ", label_style),
            Span::styled(warning.reason.clone(), value_style),
        ]),
        Line::from(vec![
            Span::styled("  Fix:     ", label_style),
            Span::styled(
                "System Settings → Privacy & Security → Full Disk Access",
                value_style,
            ),
        ]),
        Line::from(vec![Span::styled(
            "           Add and enable your terminal app, then restart looper.",
            dim_style,
        )]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(255, 170, 60)));

    frame.render_widget(Paragraph::new(lines).block(block), area);
}

pub fn draw_startup(frame: &mut ratatui::Frame, state: &StartupScreenState) {
    let area = match state.sync_warning.as_ref() {
        Some(warning) => {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(SYNC_WARNING_HEIGHT),
                    Constraint::Min(0),
                ])
                .split(frame.area());
            draw_sync_warning(frame, split[0], warning);
            split[1]
        }
        None => frame.area(),
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(16),
            Constraint::Length(2),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(area);

    let title = vec![
        Line::from("  _                                   .-''''-.    .-''''-. "),
        Line::from(" | |    ___   ___  _ __   ___ _ __  .'  .-.  '. .'  .-.  '."),
        Line::from(" | |   / _ \\ / _ \\| '_ \\ / _ \\ '__|/   /   \\   V   /   \\   \\"),
        Line::from(" | |__| (_) | (_) | |_) |  __/ |   \\   \\   /       \\   /   /"),
        Line::from(" |_____\\___/ \\___/| .__/ \\___|_|    '.  '-'  .' '.  '-'  .' "),
        Line::from("                  |_|                 '-.__.-'   '-.__.-'   "),
    ];

    frame.render_widget(
        Paragraph::new(title).alignment(Alignment::Center).style(
            Style::default()
                .fg(Color::Rgb(255, 180, 80))
                .add_modifier(Modifier::BOLD),
        ),
        chunks[1],
    );

    let spinner = spinner_frame(state.frame_count);
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![
            Span::styled(
                format!("{spinner} "),
                Style::default().fg(Color::Rgb(120, 220, 80)),
            ),
            Span::styled(&state.status, Style::default().fg(Color::White)),
        ])])
        .alignment(Alignment::Center),
        chunks[2],
    );

    let log_lines = state
        .logs
        .iter()
        .map(|line| {
            Line::from(vec![
                Span::styled("• ", Style::default().fg(Color::Rgb(255, 160, 50))),
                Span::styled(line, Style::default().fg(Color::Rgb(180, 180, 200))),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(log_lines).alignment(Alignment::Center),
        chunks[3],
    );
}

pub fn draw_history_browser(
    frame: &mut ratatui::Frame,
    panel: &HistoryPanelState,
    sync_warning: Option<&SyncWarning>,
) {
    let area = match sync_warning {
        Some(warning) => {
            let split = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(SYNC_WARNING_HEIGHT),
                    Constraint::Min(0),
                ])
                .split(frame.area());
            draw_sync_warning(frame, split[0], warning);
            split[1]
        }
        None => frame.area(),
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);

    let header = vec![
        Line::from(vec![
            Span::styled(
                "Playlist History",
                Style::default()
                    .fg(Color::Rgb(255, 180, 80))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  •  enter replay  j/k move  h/l sort  r reverse  s star  q quit",
                Style::default().fg(Color::Rgb(150, 150, 170)),
            ),
        ]),
        Line::from(vec![Span::styled(
            "No track is playing. Pick something from history to start the loop.",
            Style::default().fg(Color::Rgb(180, 180, 200)),
        )]),
    ];
    frame.render_widget(
        Paragraph::new(header).block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::Rgb(90, 90, 120))),
        ),
        chunks[0],
    );

    draw_history_table(
        frame,
        chunks[1],
        panel,
        "Played Songs",
        "Tiny jukebox historian online. It remembers everything, especially your repeats.",
    );
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![Span::styled(
            "Bare `looper` now opens here first. `looper play --url ...` still jumps straight into playback.",
            Style::default().fg(Color::Rgb(120, 120, 145)),
        )])]),
        chunks[2],
    );
}

fn draw_normal_in(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // header
            Constraint::Min(0),    // visualizer — takes all remaining space
            Constraint::Length(3), // progress bar
            Constraint::Length(3), // footer
        ])
        .split(area);

    draw_header(frame, chunks[0], state);
    draw_scatter(frame, chunks[1], state, true);
    draw_progress(frame, chunks[2], state);
    draw_footer(frame, chunks[3]);
}

fn draw_fullscreen_in(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    // Visualizer fills everything except a 1-row status strip at the bottom
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),    // scatter
            Constraint::Length(1), // micro status line
        ])
        .split(area);

    draw_scatter(frame, chunks[0], state, false);
    draw_micro_status(frame, chunks[1], state);
}

// ── Header ────────────────────────────────────────────────────────────────────

fn draw_header(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let (status_text, status_color) = if state.paused {
        ("⏸  PAUSED", Color::Yellow)
    } else {
        ("●  PLAYING", Color::Green)
    };

    let secondary_text = if state.is_playlist {
        format!("Track {}/{}", state.track_index, state.total_tracks)
    } else {
        format!("Loop #{} of ∞", state.loop_count)
    };

    let cache_badge = state.cache_status.as_ref().and_then(|status| {
        if status.complete {
            None
        } else if let Some(fraction) = status.progress.fraction() {
            Some(format!("CACHE {:>3}%", (fraction * 100.0).round() as u64))
        } else if status.progress.downloaded_bytes.is_some() {
            Some("BUFFERING".to_string())
        } else {
            None
        }
    });

    let service_badge = service_badge(&state.service);

    let text = vec![
        Line::from(vec![
            Span::styled("  ♪  ", Style::default().fg(Color::Rgb(255, 180, 80))),
            service_badge,
            Span::raw(" "),
            favorite_badge(state.is_favorite),
            Span::raw(if state.is_favorite { " " } else { "" }),
            Span::styled(
                state.filename.clone(),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(status_text, Style::default().fg(status_color)),
        ]),
        Line::from(vec![
            Span::raw("       "),
            Span::styled(secondary_text, Style::default().fg(Color::DarkGray)),
            if let Some(badge) = cache_badge {
                Span::styled(
                    format!("  {badge}"),
                    Style::default()
                        .fg(Color::Rgb(255, 180, 80))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                Span::raw("")
            },
            Span::styled(
                "  [f] fullscreen",
                Style::default().fg(Color::Rgb(80, 80, 100)),
            ),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(60, 60, 80)));

    frame.render_widget(Paragraph::new(text).block(block), area);
}

// ── Scatter visualizer ────────────────────────────────────────────────────────

fn draw_scatter(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &AppState,
    bordered: bool,
) {
    let inner = if bordered {
        let block = Block::default()
            .borders(Borders::ALL)
            .style(Style::default().fg(Color::Rgb(40, 40, 60)));
        let inner = block.inner(area);
        frame.render_widget(block, area);
        inner
    } else {
        area
    };

    let h = inner.height as usize;
    let w = inner.width as usize;
    let n = state.bands.len();

    if h == 0 || w == 0 || n == 0 {
        return;
    }

    if state.paused {
        let rows: Vec<Line> = (0..h)
            .map(|row| {
                let spans: Vec<Span> = (0..w)
                    .map(
                        |col| match paused_wave_cell(row, col, w, h, state.frame_count) {
                            Some(color) => Span::styled("·", Style::default().fg(color)),
                            None => Span::raw(" "),
                        },
                    )
                    .collect();
                Line::from(spans)
            })
            .collect();

        frame.render_widget(Paragraph::new(rows), inner);
        return;
    }

    let rows: Vec<Line> = (0..h)
        .map(|row| {
            let spans: Vec<Span> = (0..w)
                .map(|col| {
                    let band_idx = (col * n) / w;
                    let amp = state.bands[band_idx];

                    // row_ratio: 0.0 at top of area, 1.0 at bottom
                    let row_ratio = if h > 1 {
                        (h - 1 - row) as f32 / (h - 1) as f32
                    } else {
                        1.0
                    };

                    // Dots appear only below the amplitude ceiling.
                    // Density is highest at the bottom and falls off quadratically toward the top.
                    let show = if amp > 0.001 && row_ratio <= amp {
                        let t = row_ratio / amp; // 0 at ceiling, 1 at floor
                        let density = (1.0 - t) * (1.0 - t) * 0.75;
                        cell_noise(row, col, (state.frame_count / 4) as usize) < density
                    } else {
                        false
                    };

                    if show {
                        Span::styled("·", Style::default().fg(scatter_color(band_idx, n)))
                    } else {
                        Span::raw(" ")
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    frame.render_widget(Paragraph::new(rows), inner);
}

/// Per-cell pseudo-random value in [0, 1). Advances slowly with `t` (frame_count / 4)
/// to produce gentle shimmer without flickering.
fn cell_noise(row: usize, col: usize, t: usize) -> f32 {
    let mut h: usize = row
        .wrapping_mul(2246822519)
        .wrapping_add(col.wrapping_mul(3266489917))
        .wrapping_add(t.wrapping_mul(1664525));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^= h >> 16;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth warm-to-cool gradient by frequency: pink (bass) → amber → yellow → lime → cyan (treble)
fn scatter_color(band_idx: usize, total: usize) -> Color {
    let t = band_idx as f32 / (total - 1).max(1) as f32;
    let stops: [(f32, f32, f32); 5] = [
        (255.0, 80.0, 130.0), // pink  (bass)
        (255.0, 150.0, 50.0), // amber
        (220.0, 210.0, 60.0), // yellow
        (120.0, 220.0, 80.0), // lime
        (80.0, 200.0, 210.0), // cyan  (treble)
    ];
    let scaled = t * (stops.len() - 1) as f32;
    let lo = scaled.floor() as usize;
    let hi = (lo + 1).min(stops.len() - 1);
    let frac = scaled - lo as f32;
    let r = (stops[lo].0 + frac * (stops[hi].0 - stops[lo].0)).round() as u8;
    let g = (stops[lo].1 + frac * (stops[hi].1 - stops[lo].1)).round() as u8;
    let b = (stops[lo].2 + frac * (stops[hi].2 - stops[lo].2)).round() as u8;
    Color::Rgb(r, g, b)
}

fn paused_wave_cell(
    row: usize,
    col: usize,
    width: usize,
    height: usize,
    frame_count: u64,
) -> Option<Color> {
    if width < 8 || height < 4 {
        return None;
    }

    let x = if width > 1 {
        col as f32 / (width - 1) as f32
    } else {
        0.0
    };
    let y = if height > 1 {
        row as f32 / (height - 1) as f32
    } else {
        0.0
    };
    let t = frame_count as f32 * 0.04;

    let crest_center = 0.18 + 0.50 * (0.5 + 0.5 * (t * 0.35).sin());
    let crest_width = 0.18 + 0.04 * (t * 0.22).cos();
    let crest_shape = gaussian(x, crest_center, crest_width);
    let curl_shape = gaussian(x, crest_center + 0.07, crest_width * 0.55);
    let trailing_center = (crest_center - 0.28).clamp(0.12, 0.78);
    let trailing_shape = gaussian(x, trailing_center, crest_width * 1.45);
    let horizon = 0.80 + 0.015 * (t * 0.20).sin();

    let main_surface = (horizon
        - 0.34 * crest_shape
        - 0.06 * (t + x * 6.5).sin()
        - 0.02 * (t * 0.7 + x * 18.0).cos())
    .clamp(0.12, 0.95);
    let trailing_surface =
        (0.86 - 0.17 * trailing_shape - 0.03 * (t * 0.75 + x * 5.2 + 1.3).sin()).clamp(0.18, 0.97);
    let surface = main_surface.min(trailing_surface);

    let curl_center_y = (main_surface - 0.13 - 0.06 * curl_shape).clamp(0.08, 0.90);
    let curl_ring = ((x - (crest_center + 0.055)).powi(2) / (crest_width * 0.42).powi(2)
        + (y - curl_center_y).powi(2) / 0.014)
        .sqrt();

    let noise = cell_noise(row, col, (frame_count / 3) as usize);
    let deep_water = Color::Rgb(22, 32, 68);
    let mid_water = Color::Rgb(66, 92, 138);
    let foam = Color::Rgb(239, 236, 224);

    let wave_depth = (surface - y).max(0.0);
    if wave_depth > 0.0 {
        let density = (0.20 + wave_depth * 2.3 + crest_shape * 0.18).clamp(0.0, 0.96);
        if noise < density {
            if (surface - y) < 0.055 + crest_shape * 0.04 {
                return Some(mid_water);
            }
            return Some(deep_water);
        }
    }

    let foam_band = (y - surface).abs();
    if foam_band < 0.03 + crest_shape * 0.03 {
        let density = (0.56 - foam_band * 10.0 + crest_shape * 0.30).clamp(0.0, 0.96);
        if noise < density {
            return Some(foam);
        }
    }

    if curl_ring > 0.74 && curl_ring < 1.04 && x > crest_center - 0.04 && x < crest_center + 0.20 {
        let density = (0.68 - (curl_ring - 0.89).abs() * 2.8 + curl_shape * 0.24).clamp(0.0, 0.92);
        if noise < density {
            return Some(foam);
        }
    }

    let spray_height = (surface - 0.19 - 0.10 * curl_shape).clamp(0.0, 1.0);
    if y < spray_height && x > crest_center - 0.01 && x < crest_center + 0.18 {
        let spray_bias = gaussian(x, crest_center + 0.08, 0.08);
        let density = (spray_bias * (spray_height - y) * 6.0 - 0.08).clamp(0.0, 0.42);
        if noise < density {
            return Some(foam);
        }
    }

    None
}

fn gaussian(x: f32, center: f32, width: f32) -> f32 {
    let w = width.max(0.0001);
    let z = (x - center) / w;
    (-0.5 * z * z).exp()
}

// ── Progress bar ──────────────────────────────────────────────────────────────

fn draw_progress(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let elapsed = state.elapsed();

    let time_label = match state.duration {
        Some(dur) if dur.as_secs() > 0 => {
            let e = elapsed.as_secs();
            let t = dur.as_secs();
            format!(" {}:{:02} / {}:{:02} ", e / 60, e % 60, t / 60, t % 60)
        }
        _ => {
            let e = elapsed.as_secs();
            format!(" {}:{:02} / --:-- ", e / 60, e % 60)
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(60, 60, 80)));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    if inner.width < 4 {
        return;
    }

    // Reserve right side for the time label
    let label_len = time_label.len() as u16;
    let track_width = inner.width.saturating_sub(label_len) as usize;

    let ratio = match state.duration {
        Some(dur) if dur.as_secs_f64() > 0.0 => {
            (elapsed.as_secs_f64() / dur.as_secs_f64()).min(1.0) as f32
        }
        _ => 0.0,
    };

    let filled = (ratio * track_width as f32).round() as usize;

    // Build the track: filled portion, knob, empty portion
    let track: String = (0..track_width)
        .map(|i| {
            if i + 1 == filled.max(1) {
                PROGRESS_KNOB
            } else if i < filled {
                '━'
            } else {
                '─'
            }
        })
        .collect();

    let line = Line::from(vec![
        Span::styled(track, Style::default().fg(Color::Rgb(255, 160, 50))),
        Span::styled(time_label, Style::default().fg(Color::Rgb(180, 180, 200))),
    ]);

    frame.render_widget(Paragraph::new(vec![line]), inner);
}

// ── Footer ────────────────────────────────────────────────────────────────────

fn draw_footer(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let text = vec![Line::from(vec![
        Span::styled("[Space]", Style::default().fg(Color::Rgb(255, 160, 50))),
        Span::raw(" Pause/Resume   "),
        Span::styled("[f]", Style::default().fg(Color::Rgb(255, 160, 50))),
        Span::raw(" Fullscreen   "),
        Span::styled("[s]", Style::default().fg(Color::Rgb(255, 160, 50))),
        Span::raw(" Favorite   "),
        Span::styled("[p]", Style::default().fg(Color::Rgb(255, 160, 50))),
        Span::raw(" Played   "),
        Span::styled("[q]", Style::default().fg(Color::Rgb(255, 160, 50))),
        Span::raw(" Quit"),
    ])];

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(60, 60, 80)));

    frame.render_widget(
        Paragraph::new(text)
            .block(block)
            .alignment(Alignment::Center),
        area,
    );
}

// ── Fullscreen micro status ───────────────────────────────────────────────────

fn draw_micro_status(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let elapsed = state.elapsed().as_secs();
    let time_str = match state.duration {
        Some(dur) if dur.as_secs() > 0 => {
            let t = dur.as_secs();
            format!(
                "{}:{:02}/{}:{:02}",
                elapsed / 60,
                elapsed % 60,
                t / 60,
                t % 60
            )
        }
        _ => format!("{}:{:02}", elapsed / 60, elapsed % 60),
    };

    let status = if state.paused {
        "⏸ PAUSED"
    } else {
        "● PLAYING"
    };
    let secondary_info = if state.is_playlist {
        format!("Track {}/{}", state.track_index, state.total_tracks)
    } else {
        format!("Loop #{}", state.loop_count)
    };
    let cache_info = state.cache_status.as_ref().and_then(|status| {
        if status.complete {
            None
        } else if let Some(fraction) = status.progress.fraction() {
            Some(format!("CACHE {}%", (fraction * 100.0).round() as u64))
        } else if status.progress.downloaded_bytes.is_some() {
            Some("BUFFERING".to_string())
        } else {
            None
        }
    });

    let mut spans = vec![
        Span::styled(" ♪ ", Style::default().fg(Color::Rgb(255, 180, 80))),
        service_badge(&state.service),
        Span::raw(" "),
        favorite_badge(state.is_favorite),
        Span::raw(if state.is_favorite { " " } else { "" }),
        Span::styled(&state.filename, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(time_str, Style::default().fg(Color::Rgb(180, 180, 200))),
        Span::raw("  "),
        Span::styled(secondary_info, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(Color::Rgb(120, 220, 80))),
    ];
    if let Some(cache_info) = cache_info {
        spans.push(Span::raw("  "));
        spans.push(Span::styled(
            cache_info,
            Style::default().fg(Color::Rgb(255, 180, 80)),
        ));
    }
    spans.push(Span::styled(
        "  [f] exit fullscreen",
        Style::default().fg(Color::Rgb(80, 80, 100)),
    ));

    let line = Line::from(spans);

    frame.render_widget(Paragraph::new(vec![line]), area);
}

fn service_badge(service: &Option<String>) -> Span<'static> {
    match service.as_deref() {
        Some("YouTube") => badge_span("YT", Color::Rgb(255, 90, 90)),
        Some("SoundCloud") => badge_span("SC", Color::Rgb(255, 150, 50)),
        Some("HypeM") => badge_span("HM", Color::Rgb(80, 200, 210)),
        Some("Online") => badge_span("ON", Color::Rgb(180, 180, 200)),
        _ => Span::raw(""),
    }
}

fn badge_span(label: &'static str, color: Color) -> Span<'static> {
    Span::styled(
        format!("[{label}]"),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )
}

fn favorite_badge(is_favorite: bool) -> Span<'static> {
    if is_favorite {
        Span::styled(
            "[*]",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw("")
    }
}

fn draw_history_panel(frame: &mut ratatui::Frame, _state: &AppState, panel: &HistoryPanelState) {
    let area = centered_rect(88, 72, frame.area());
    frame.render_widget(Clear, area);
    draw_history_table(
        frame,
        area,
        panel,
        "Played Songs",
        "Tiny jukebox historian online. It remembers everything, especially your repeats.",
    );
}

fn draw_history_table(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    panel: &HistoryPanelState,
    title_label: &str,
    footer_text: &str,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(90, 90, 120)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(inner);

    let title = format!(
        "{title_label}  •  sort: {} {}",
        panel.sort_field.label(),
        if panel.descending { "↓" } else { "↑" }
    );
    let controls = if area == frame.area() {
        "  •  j/k move  h/l sort  r reverse  s star  enter replay  q quit"
    } else {
        "  •  j/k move  h/l sort  r reverse  s star  enter replay  p/esc close"
    };
    let header = vec![Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(controls, Style::default().fg(Color::Rgb(150, 150, 170))),
    ])];
    frame.render_widget(Paragraph::new(header), chunks[0]);

    if panel.rows.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![Line::from(vec![Span::styled(
                "No songs played yet. Play something once and it will show up here.",
                Style::default().fg(Color::Rgb(180, 180, 200)),
            )])]),
            chunks[1],
        );
    } else {
        let widths = [
            Constraint::Length(1),  // favorite marker
            Constraint::Min(20),    // title — flexes with terminal width
            Constraint::Length(10), // platform
            Constraint::Length(16), // last played: YYYY-MM-DD HH:MM
            Constraint::Length(8),  // total time, right-aligned
            Constraint::Length(5),  // play count, right-aligned
        ];

        let header_color = Color::Rgb(255, 180, 80);
        let header_style = Style::default()
            .fg(header_color)
            .add_modifier(Modifier::BOLD);
        let header_row = Row::new(vec![
            Cell::from(""),
            Cell::from("Title"),
            Cell::from("Platform"),
            Cell::from("Last Played"),
            Cell::from(Line::from("Time").alignment(Alignment::Right)),
            Cell::from(Line::from("Plays").alignment(Alignment::Right)),
        ])
        .style(header_style)
        .bottom_margin(1);

        let dim_style = Style::default().fg(Color::Rgb(170, 175, 200));
        let body_rows: Vec<Row> = panel
            .rows
            .iter()
            .map(|row| {
                let marker = if row.is_favorite { "★" } else { " " };
                let last_played = format_timestamp(row.last_played_at);
                let total_time = format_total_play_time(row.total_play_seconds);
                Row::new(vec![
                    Cell::from(marker).style(Style::default().fg(Color::Yellow)),
                    Cell::from(row.title.clone()),
                    Cell::from(row.platform.clone()).style(dim_style),
                    Cell::from(last_played).style(dim_style),
                    Cell::from(Line::from(total_time).alignment(Alignment::Right)),
                    Cell::from(
                        Line::from(format!("{}", row.play_count)).alignment(Alignment::Right),
                    ),
                ])
            })
            .collect();

        let table = Table::new(body_rows, widths)
            .header(header_row)
            .column_spacing(2)
            .style(Style::default().fg(Color::Rgb(210, 210, 220)))
            .row_highlight_style(
                Style::default()
                    .bg(Color::Rgb(45, 45, 65))
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            );

        let mut state = TableState::default();
        state.select(Some(panel.selected));
        frame.render_stateful_widget(table, chunks[1], &mut state);
    }
    frame.render_widget(
        Paragraph::new(vec![Line::from(vec![Span::styled(
            footer_text,
            Style::default().fg(Color::Rgb(120, 120, 145)),
        )])]),
        chunks[2],
    );
}

fn centered_rect(
    percent_x: u16,
    percent_y: u16,
    area: ratatui::layout::Rect,
) -> ratatui::layout::Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn format_timestamp(timestamp: i64) -> String {
    Local
        .timestamp_opt(timestamp, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn format_total_play_time(total_seconds: i64) -> String {
    let total_seconds = total_seconds.max(0) as u64;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn spinner_frame(frame_count: u64) -> char {
    const FRAMES: [char; 4] = ['◐', '◓', '◑', '◒'];
    FRAMES[((frame_count / 6) as usize) % FRAMES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn buffer_contains(terminal: &Terminal<TestBackend>, needle: &str) -> bool {
        let buffer = terminal.backend().buffer();
        for y in 0..buffer.area.height {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer.get(x, y).symbol().chars().next().unwrap_or(' '))
                .collect();
            if line.contains(needle) {
                return true;
            }
        }
        false
    }

    fn sample_warning() -> SyncWarning {
        SyncWarning {
            attempted_path: PathBuf::from(
                "/Users/test/Library/Mobile Documents/com~apple~CloudDocs/looper/looper.sqlite3",
            ),
            reason: "failed to open looper database: authorization denied".into(),
        }
    }

    #[test]
    fn startup_renders_sync_warning_banner() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = StartupScreenState {
            status: "loading".into(),
            logs: vec!["warming up".into()],
            frame_count: 0,
            sync_warning: Some(sample_warning()),
        };
        terminal.draw(|frame| draw_startup(frame, &state)).unwrap();
        assert!(buffer_contains(&terminal, "History sync disabled"));
        assert!(buffer_contains(&terminal, "Full Disk Access"));
        assert!(buffer_contains(&terminal, "looper.sqlite3"));
    }

    #[test]
    fn history_browser_renders_sync_warning_banner() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let panel = HistoryPanelState {
            rows: vec![],
            selected: 0,
            sort_field: HistorySortField::TimePlayed,
            descending: true,
        };
        let warning = sample_warning();
        terminal
            .draw(|frame| draw_history_browser(frame, &panel, Some(&warning)))
            .unwrap();
        assert!(buffer_contains(&terminal, "History sync disabled"));
        assert!(buffer_contains(&terminal, "Full Disk Access"));
    }

    #[test]
    fn startup_without_warning_omits_banner() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let state = StartupScreenState {
            status: "loading".into(),
            logs: vec!["warming up".into()],
            frame_count: 0,
            sync_warning: None,
        };
        terminal.draw(|frame| draw_startup(frame, &state)).unwrap();
        assert!(!buffer_contains(&terminal, "History sync disabled"));
    }
}
