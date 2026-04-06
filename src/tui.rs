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
    widgets::{Block, Borders, Gauge, Paragraph},
};
use std::{
    io::{Stdout, stdout},
    time::{Duration, Instant},
};

pub struct AppState {
    pub filename: String,
    pub duration: Option<Duration>,
    pub paused: bool,
    pub loop_count: u64,
    pub loop_start: Instant,
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
    let area = frame.size();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4), // header
            Constraint::Length(3), // progress
            Constraint::Length(5), // visualizer
            Constraint::Length(3), // footer
        ])
        .split(area);

    draw_header(frame, chunks[0], state);
    draw_progress(frame, chunks[1], state);
    draw_visualizer(frame, chunks[2], state);
    draw_footer(frame, chunks[3]);
}

fn draw_header(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let (status_text, status_color) = if state.paused {
        ("⏸  PAUSED", Color::Yellow)
    } else {
        ("●  PLAYING", Color::Green)
    };

    let loop_text = format!("Loop #{} of ∞", state.loop_count);

    let text = vec![
        Line::from(vec![
            Span::styled("  ♪  ", Style::default().fg(Color::Cyan)),
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
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(text).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_progress(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let elapsed = if state.paused {
        // freeze display at last known position
        state.loop_start.elapsed()
    } else {
        state.loop_start.elapsed()
    };

    let (ratio, label) = match state.duration {
        Some(dur) if dur.as_secs_f64() > 0.0 => {
            let r = (elapsed.as_secs_f64() / dur.as_secs_f64()).min(1.0);
            let elapsed_s = elapsed.as_secs();
            let total_s = dur.as_secs();
            let label = format!(
                "  {}:{:02} / {}:{:02}",
                elapsed_s / 60,
                elapsed_s % 60,
                total_s / 60,
                total_s % 60
            );
            (r, label)
        }
        _ => (0.0, "  --:-- / --:--".to_string()),
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .style(Style::default().fg(Color::DarkGray)),
        )
        .gauge_style(Style::default().fg(Color::Cyan))
        .ratio(ratio)
        .label(label);

    frame.render_widget(gauge, area);
}

fn draw_visualizer(frame: &mut ratatui::Frame, area: ratatui::layout::Rect, state: &AppState) {
    let inner_width = area.width.saturating_sub(2) as usize; // subtract borders
    let bar_chars = [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

    let elapsed = state.loop_start.elapsed().as_secs_f64();
    // freeze animation when paused
    let t = if state.paused { 0.0 } else { elapsed };

    let bars: String = (0..inner_width)
        .map(|i| {
            let phase = i as f64 * 0.45;
            let wave1 = (t * 3.2 + phase).sin().abs();
            let wave2 = (t * 1.7 + phase * 1.3).sin().abs() * 0.5;
            let height = ((wave1 + wave2) / 1.5).min(1.0);
            let idx = (height * (bar_chars.len() - 1) as f64).round() as usize;
            bar_chars[idx]
        })
        .collect();

    let text = vec![
        Line::from(""),
        Line::from(Span::styled(bars, Style::default().fg(Color::Cyan))),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(text).block(block).alignment(Alignment::Left);
    frame.render_widget(paragraph, area);
}

fn draw_footer(frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
    let text = vec![Line::from(vec![
        Span::styled("[Space]", Style::default().fg(Color::Cyan)),
        Span::raw(" Pause/Resume   "),
        Span::styled("[q]", Style::default().fg(Color::Cyan)),
        Span::raw(" Quit"),
    ])];

    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().fg(Color::DarkGray));

    let paragraph = Paragraph::new(text)
        .block(block)
        .alignment(Alignment::Center);
    frame.render_widget(paragraph, area);
}
