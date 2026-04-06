use color_eyre::eyre::Result;
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
};
use std::{
    io::{Stdout, stdout},
    time::{Duration, Instant},
};

pub const N_BANDS: usize = 32;

pub struct AppState {
    pub filename: String,
    pub duration: Option<Duration>,
    pub paused: bool,
    pub loop_count: u64,
    pub loop_start: Instant,
    pub bands: Vec<f32>,
    pub prev_bands: Vec<f32>,
    pub fullscreen: bool,
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

    let loop_text = format!("Loop #{} of ∞", state.loop_count);

    let text = vec![
        Line::from(vec![
            Span::styled("  ♪  ", Style::default().fg(Color::Rgb(255, 180, 80))),
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
            Span::styled(loop_text, Style::default().fg(Color::DarkGray)),
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
                        cell_noise(row, col) < density
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

/// Stable per-cell pseudo-random value in [0, 1). Same (row, col) always gives
/// the same value so the dot pattern doesn't flicker between frames.
fn cell_noise(row: usize, col: usize) -> f32 {
    let mut h: usize = row.wrapping_mul(2246822519).wrapping_add(col.wrapping_mul(3266489917));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^= h >> 16;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Warm-to-cool gradient by frequency: pink (bass) → amber → yellow → cyan (treble)
fn scatter_color(band_idx: usize, total: usize) -> Color {
    let t = band_idx as f32 / total as f32;
    if t < 0.20 {
        Color::Rgb(255, 80, 130) // hot pink — bass
    } else if t < 0.40 {
        Color::Rgb(255, 150, 50) // amber — low-mid
    } else if t < 0.60 {
        Color::Rgb(220, 210, 60) // yellow — mid
    } else if t < 0.80 {
        Color::Rgb(120, 220, 80) // lime — high-mid
    } else {
        Color::Rgb(80, 200, 210) // cyan — treble
    }
}

// ── Progress bar ──────────────────────────────────────────────────────────────

fn draw_progress(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let elapsed = state.loop_start.elapsed();

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
                '●' // position knob
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
    let elapsed = state.loop_start.elapsed().as_secs();
    let time_str = match state.duration {
        Some(dur) if dur.as_secs() > 0 => {
            let t = dur.as_secs();
            format!("{}:{:02}/{}:{:02}", elapsed / 60, elapsed % 60, t / 60, t % 60)
        }
        _ => format!("{}:{:02}", elapsed / 60, elapsed % 60),
    };

    let status = if state.paused { "⏸ PAUSED" } else { "● PLAYING" };
    let loop_info = format!("Loop #{}", state.loop_count);

    let line = Line::from(vec![
        Span::styled(" ♪ ", Style::default().fg(Color::Rgb(255, 180, 80))),
        Span::styled(&state.filename, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled(time_str, Style::default().fg(Color::Rgb(180, 180, 200))),
        Span::raw("  "),
        Span::styled(loop_info, Style::default().fg(Color::DarkGray)),
        Span::raw("  "),
        Span::styled(status, Style::default().fg(Color::Rgb(120, 220, 80))),
        Span::styled("  [f] exit fullscreen", Style::default().fg(Color::Rgb(80, 80, 100))),
    ]);

    frame.render_widget(Paragraph::new(vec![line]), area);
}
