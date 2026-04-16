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
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{
    io::{stdout, Stdout},
    time::{Duration, Instant},
};

use crate::download::{
    format_bytes, format_eta, format_speed, CacheStatus, LoadingPhase, LoadingState,
};

pub const N_BANDS: usize = 32;
const PROGRESS_KNOB: char = '•';

pub struct AppState {
    pub filename: String,
    pub service: Option<String>,
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

pub fn draw(frame: &mut ratatui::Frame, state: &AppState) {
    if state.fullscreen {
        draw_fullscreen(frame, state);
    } else {
        draw_normal(frame, state);
    }
}

pub fn draw_loading(frame: &mut ratatui::Frame, state: &LoadingState) {
    let area = frame.size();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);

    draw_loading_title(frame, chunks[1], state);
    draw_loading_bar(frame, chunks[2], state);
    draw_loading_meta(frame, chunks[3], state);
    draw_loading_ambient(frame, chunks[4], state);
    draw_loading_footer(frame, chunks[5]);
}

fn draw_normal(frame: &mut ratatui::Frame, state: &AppState) {
    let area = frame.size();

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

fn draw_fullscreen(frame: &mut ratatui::Frame, state: &AppState) {
    let area = frame.size();

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

fn draw_loading_title(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &LoadingState,
) {
    let subtitle = if state.is_playlist {
        format!(
            "Track {}/{}  •  {}",
            state.track_index, state.total_tracks, state.service
        )
    } else {
        format!("Single Track  •  {}", state.service)
    };

    let status = match &state.phase {
        LoadingPhase::Resolving => "Preparing audio",
        LoadingPhase::Downloading => "Downloading",
        LoadingPhase::Finalizing => "Finalizing",
        LoadingPhase::Ready => "Ready",
        LoadingPhase::Error(_) => "Download failed",
    };

    let lines = vec![
        Line::from(vec![Span::styled(
            state.title.clone(),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(subtitle, Style::default().fg(Color::Rgb(255, 180, 80))),
            Span::raw("  "),
            Span::styled(status, Style::default().fg(Color::Rgb(180, 180, 200))),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(60, 60, 80)));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_loading_bar(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &LoadingState) {
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::Rgb(60, 60, 80)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 4 {
        return;
    }

    let ratio = state.progress.fraction().unwrap_or_else(|| {
        let shimmer = (state.frame_count % 20) as f64 / 19.0;
        shimmer.clamp(0.05, 0.95)
    });
    let width = inner.width.saturating_sub(2) as usize;
    let filled = (ratio * width as f64).round() as usize;
    let bar: String = (0..width)
        .map(|idx| {
            if idx + 1 == filled.max(1) {
                PROGRESS_KNOB
            } else if idx < filled {
                '━'
            } else {
                '─'
            }
        })
        .collect();
    let label = if let Some(fraction) = state.progress.fraction() {
        format!("{:>3}%", (fraction * 100.0).round() as u64)
    } else {
        "LIVE".to_string()
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(bar, Style::default().fg(Color::Rgb(255, 160, 50))),
            Span::raw(" "),
            Span::styled(label, Style::default().fg(Color::Rgb(220, 220, 240))),
        ])),
        inner,
    );
}

fn draw_loading_meta(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &LoadingState,
) {
    let downloaded = state
        .progress
        .downloaded_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "--".to_string());
    let total = state
        .progress
        .total_bytes
        .map(format_bytes)
        .unwrap_or_else(|| "--".to_string());
    let speed = format_speed(state.progress.speed_bytes_per_sec);
    let eta = format_eta(state.progress.eta_seconds);
    let detail = match &state.phase {
        LoadingPhase::Resolving => "Inspecting remote media metadata".to_string(),
        LoadingPhase::Downloading => "Building local cache for stable playback".to_string(),
        LoadingPhase::Finalizing => "Converting audio and preparing handoff".to_string(),
        LoadingPhase::Ready => "Starting playback".to_string(),
        LoadingPhase::Error(message) => message.clone(),
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("Transferred ", Style::default().fg(Color::DarkGray)),
            Span::styled(downloaded, Style::default().fg(Color::White)),
            Span::styled(" / ", Style::default().fg(Color::DarkGray)),
            Span::styled(total, Style::default().fg(Color::White)),
            Span::styled("    Speed ", Style::default().fg(Color::DarkGray)),
            Span::styled(speed, Style::default().fg(Color::White)),
            Span::styled("    ETA ", Style::default().fg(Color::DarkGray)),
            Span::styled(eta, Style::default().fg(Color::White)),
        ]),
        Line::from(vec![Span::styled(
            detail,
            Style::default().fg(Color::Rgb(160, 160, 190)),
        )]),
    ];

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_loading_ambient(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    state: &LoadingState,
) {
    let height = area.height as usize;
    let width = area.width as usize;
    if height == 0 || width == 0 {
        return;
    }

    let lines: Vec<Line> = (0..height)
        .map(|row| {
            let spans: Vec<Span> = (0..width)
                .map(|col| {
                    let noise = cell_noise(row, col, (state.frame_count / 3) as usize);
                    let threshold = 0.92 - (row as f32 / height.max(1) as f32) * 0.25;
                    if noise > threshold {
                        let color = scatter_color((col % N_BANDS).min(N_BANDS - 1), N_BANDS);
                        Span::styled("·", Style::default().fg(color))
                    } else {
                        Span::raw(" ")
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_loading_footer(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("[q]", Style::default().fg(Color::Rgb(255, 160, 50))),
            Span::raw(" Cancel   "),
            Span::styled("[Ctrl-C]", Style::default().fg(Color::Rgb(255, 160, 50))),
            Span::raw(" Quit"),
        ]))
        .alignment(Alignment::Center),
        area,
    );
}
