//! Animated "dither fade" startup logo.
//!
//! The `looper` wordmark resolves out of static as the track buffers: at 0%
//! the logo is pure noise, at 100% it's a clean block wordmark. This ties the
//! splash directly to the app's actual job — a signal tuning into focus.
//!
//! Rendering is a pure function of `frame_count` (for shimmer) and the download
//! `fraction` (for how much has resolved), so it has no state of its own and
//! drops straight into the existing startup render path.

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Wordmark bitmap: `#` is ink (part of the letters), everything else is field.
/// Five rows of 3-wide block letters, single-space separated: L O O P E R.
const LOGO: [&str; 5] = [
    "#   ### ### ### ### ###",
    "#   # # # # # # #   # #",
    "#   # # # # ### ##  ## ",
    "#   # # # # #   #   # #",
    "### ### ### #   ### # #",
];

/// Columns of static padding on each side of the word, so the noise has room
/// to burn away from the edges inward as the logo resolves.
const FIELD_PAD: usize = 10;

/// Static ramp, sparse to dense. Picked per-cell by the shimmer value.
const SHADES: [char; 3] = ['░', '▒', '▓'];

/// How far past its reveal threshold a cell still counts as "just locked in".
/// Cells in this band are the resolving wavefront and flash bright.
const FLASH_WINDOW: f32 = 0.08;

/// Build the logo as ratatui lines, ready to drop into a centered `Paragraph`.
///
/// `fraction` is the buffer/download progress in `0.0..=1.0`. `None` means the
/// length is unknown (indeterminate) — the word stays solid with a faint
/// drifting hiss in the field instead of resolving.
pub fn dither_logo(frame_count: u64, fraction: Option<f64>) -> Vec<Line<'static>> {
    let logo_w = LOGO[0].len();
    let width = logo_w + FIELD_PAD * 2;

    (0..LOGO.len())
        .map(|row| {
            let spans = (0..width)
                .map(|col| {
                    let is_ink = col
                        .checked_sub(FIELD_PAD)
                        .and_then(|c| LOGO[row].as_bytes().get(c))
                        .map(|b| *b == b'#')
                        .unwrap_or(false);
                    cell_span(is_ink, row, col, width, frame_count, fraction)
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

fn cell_span(
    is_ink: bool,
    row: usize,
    col: usize,
    width: usize,
    frame: u64,
    fraction: Option<f64>,
) -> Span<'static> {
    match fraction {
        // Determinate: each cell has a fixed reveal threshold, so the word
        // dissolves in raggedly (grain order) rather than wiping uniformly.
        Some(p) => {
            let p = p.clamp(0.0, 1.0) as f32;
            let threshold = reveal_threshold(row, col);
            if p < threshold {
                static_span(is_ink, row, col, frame)
            } else if is_ink && p < 1.0 && p - threshold < FLASH_WINDOW && flickers(row, col, frame) {
                // Freshly crossed the threshold: pop white-hot for a frame
                // before settling into the wordmark color.
                flash_span()
            } else {
                resolved_span(is_ink, col, width)
            }
        }
        // Indeterminate: solid word + ambient hiss in the surrounding field.
        None => {
            if is_ink {
                resolved_span(true, col, width)
            } else if noise01(row, col, (frame / 4) as usize) < 0.06 {
                Span::styled("░", Style::default().fg(Color::Rgb(70, 70, 95)))
            } else {
                Span::raw(" ")
            }
        }
    }
}

/// A cell that has locked into its final state: a colored block for ink,
/// empty space for field.
fn resolved_span(is_ink: bool, col: usize, width: usize) -> Span<'static> {
    if is_ink {
        Span::styled(
            "█",
            Style::default()
                .fg(ink_color(col, width))
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::raw(" ")
    }
}

/// White-hot block for a cell at the resolving wavefront.
fn flash_span() -> Span<'static> {
    Span::styled(
        "█",
        Style::default()
            .fg(Color::Rgb(255, 246, 230))
            .add_modifier(Modifier::BOLD),
    )
}

/// Per-frame twinkle gate so the wavefront sparkles instead of holding a
/// steady bright band.
fn flickers(row: usize, col: usize, frame: u64) -> bool {
    noise01(row, col, frame as usize) > 0.5
}

/// A cell still buried in static. Ink cells get a warmer tint so the word's
/// shape faintly pre-echoes through the noise before it resolves.
fn static_span(is_ink: bool, row: usize, col: usize, frame: u64) -> Span<'static> {
    let shimmer = noise01(row, col, (frame / 3) as usize);
    let idx = ((shimmer * SHADES.len() as f32) as usize).min(SHADES.len() - 1);
    let color = if is_ink {
        Color::Rgb(150, 120, 90)
    } else {
        Color::Rgb(70, 70, 95)
    };
    Span::styled(SHADES[idx].to_string(), Style::default().fg(color))
}

/// Warm amber→pink gradient across the wordmark, echoing the scatter palette.
fn ink_color(col: usize, width: usize) -> Color {
    let t = if width > 1 {
        col as f32 / (width - 1) as f32
    } else {
        0.0
    };
    let g = (180.0 + t * (110.0 - 180.0)) as u8;
    let b = (80.0 + t * (120.0 - 80.0)) as u8;
    Color::Rgb(255, g, b)
}

/// Fixed per-cell reveal point in `0.0..1.0`. Stable across frames so the
/// dissolve order doesn't jitter as progress climbs.
fn reveal_threshold(row: usize, col: usize) -> f32 {
    hash(row, col, 0) as f32 / u32::MAX as f32
}

/// Per-cell noise that drifts with `t`, for the shimmer in unresolved static.
fn noise01(row: usize, col: usize, t: usize) -> f32 {
    hash(row, col, t as u32) as f32 / u32::MAX as f32
}

fn hash(row: usize, col: usize, t: u32) -> u32 {
    let mut h = (row as u32)
        .wrapping_mul(2246822519)
        .wrapping_add((col as u32).wrapping_mul(3266489917))
        .wrapping_add(t.wrapping_mul(1664525));
    h ^= h >> 13;
    h = h.wrapping_mul(1274126177);
    h ^= h >> 16;
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_text(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn dimensions_are_stable() {
        let lines = dither_logo(0, Some(0.5));
        assert_eq!(lines.len(), LOGO.len());
        let width = LOGO[0].len() + FIELD_PAD * 2;
        for line in &lines {
            assert_eq!(row_text(&line).chars().count(), width);
        }
    }

    #[test]
    fn fully_resolved_is_clean_wordmark() {
        // At 100% every ink cell is a solid block and the field is blank.
        let lines = dither_logo(0, Some(1.0));
        for (row, line) in lines.iter().enumerate() {
            let text = row_text(line);
            let inked = &LOGO[row].as_bytes();
            for (col, ch) in text.chars().enumerate() {
                let is_ink = col
                    .checked_sub(FIELD_PAD)
                    .and_then(|c| inked.get(c))
                    .map(|b| *b == b'#')
                    .unwrap_or(false);
                if is_ink {
                    assert_eq!(ch, '█');
                } else {
                    assert_eq!(ch, ' ');
                }
            }
        }
    }

    #[test]
    fn zero_progress_has_no_solid_blocks() {
        // At 0% nothing has resolved yet — it's all static.
        let lines = dither_logo(7, Some(0.0));
        for line in &lines {
            assert!(!row_text(line).contains('█'));
        }
    }
}
