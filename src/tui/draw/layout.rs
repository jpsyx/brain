//! Pure-ish drawing helpers shared across the panels: the logical→visual row
//! offset table (for wrapped-line scrolling/highlighting), the selection-band
//! rect, the fg-brightening pass, the flash line, and modal centering.

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::tui::*;

/// Build the logical→visual row offset table. `heights[i]` is the number of
/// wrapped rows logical line `i` occupies once the Paragraph wraps it at the
/// content width. Returns a prefix sum of length `heights.len() + 1`: entry
/// `i` is the first visual row of logical line `i`, and the final entry is
/// the total visual row count. This is the bridge between the logical-line
/// bookkeeping (`task_line_ranges`, built width-agnostically) and the
/// wrapped rows the Paragraph actually paints.
#[cfg(test)]
pub(crate) fn visual_row_offsets(heights: &[u16]) -> Vec<u16> {
    let mut acc: u16 = 0;
    let mut out = Vec::with_capacity(heights.len() + 1);
    out.push(0);
    for &h in heights {
        acc = acc.saturating_add(h);
        out.push(acc);
    }
    out
}

/// Map a logical line range to its visual (wrapped) row range via `offsets`
/// (as built by [`visual_row_offsets`]). Indices are clamped into the table
/// so a stale range (computed before the latest rebuild) can't panic.
#[cfg(test)]
pub(crate) fn visual_range(
    offsets: &[u16],
    logical: std::ops::Range<usize>,
) -> std::ops::Range<u16> {
    if offsets.is_empty() {
        return 0..0;
    }
    let last = offsets.len() - 1;
    let start = offsets[logical.start.min(last)];
    let end = offsets[logical.end.min(last)];
    start..end
}

/// Blend an RGB color 80% toward its current value, 20% toward white.
/// Non-RGB colors (named, indexed, reset) pass through unchanged — we
/// don't want to override a deliberately styled crossed-out / dimmed
/// cell with something it can't represent.
pub(crate) fn brighten_color(c: Color) -> Color {
    // Each channel: (orig * 8 + 255 * 2 + 5) / 10 → 80/20 blend with
    // white, rounded. Max value of the dividend is 255*8 + 510 + 5 =
    // 2555, divided by 10 = 255 — fits in u8 cleanly.
    #[allow(clippy::cast_possible_truncation)]
    fn blend(x: u8) -> u8 {
        ((u16::from(x) * 8 + 255 * 2 + 5) / 10) as u8
    }
    match c {
        Color::Rgb(r, g, b) => Color::Rgb(blend(r), blend(g), blend(b)),
        other => other,
    }
}

/// Walk every cell in `band` and brighten its foreground color. Called
/// after the body Paragraph has written its span colors so we operate
/// on the actual final fg per cell.
pub(crate) fn brighten_band_text(f: &mut Frame, band: Rect) {
    let buf = f.buffer_mut();
    for y in band.y..band.y.saturating_add(band.height) {
        for x in band.x..band.x.saturating_add(band.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                let brighter = brighten_color(cell.fg);
                cell.set_fg(brighter);
            }
        }
    }
}

pub(crate) fn flash_line(flash: &FlashKind) -> Line<'static> {
    match flash {
        FlashKind::Info(msg) => Line::from(vec![
            Span::raw(" "),
            Span::styled(
                msg.clone(),
                Style::default()
                    .fg(Color::Rgb(158, 206, 106)) // green
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        FlashKind::Error(msg) => Line::from(vec![
            Span::raw(" "),
            Span::styled(
                msg.clone(),
                Style::default()
                    .fg(Color::Rgb(247, 118, 142)) // pink-red
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    }
}

/// Center a sub-Rect of the given width/height inside `area`, clamped so
/// the modal always fits even on tiny terminals.
pub(crate) fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + (area.height.saturating_sub(h)) / 2;
    Rect {
        x,
        y,
        width: w,
        height: h,
    }
}
