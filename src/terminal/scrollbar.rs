//! Scrollbar painting for TUI mode.
//!
//! The frame model derives every bar's geometry; this module only
//! stamps cells. Glyphs and arrow visibility come from the typed
//! scrollbar config; the painted span comes from the bar's strip
//! helpers, so no caller recomputes arrow positions.

use crate::terminal::display::{Cell, CellRole, Color, ScreenBuffer};

/// Render a vertical scrollbar from the unified model geometry.
///
/// Paints the bar's strip: arrows at the strip ends when the config
/// shows them, the arrow-free track between. All positions come from
/// [`crate::core::geometry::ScrollbarGeom`]; nothing here recomputes
/// them.
pub fn render_vertical(
    buffer: &mut ScreenBuffer,
    geom: &crate::core::geometry::ScrollbarGeom,
    track_fg: Color,
    thumb_fg: Color,
    track_bg: Color,
) {
    use crate::config::scrollbar::{GLYPHS, SHOW_ARROWS};

    let start = geom.strip_start();
    let len = geom.strip_len();
    if len == 0 {
        return;
    }
    for y in 0..len {
        let screen_y = (start + y) as u16;
        let (ch, fg, bg) = if SHOW_ARROWS && y == 0 {
            (GLYPHS.arrow_up, thumb_fg, track_bg)
        } else if SHOW_ARROWS && y + 1 == len {
            (GLYPHS.arrow_down, thumb_fg, track_bg)
        } else {
            let track_y = y.saturating_sub(usize::from(SHOW_ARROWS));
            if track_y >= geom.thumb_start && track_y < geom.thumb_start + geom.thumb_len {
                (GLYPHS.thumb, thumb_fg, track_bg)
            } else {
                (GLYPHS.track_v, track_fg, track_bg)
            }
        };
        buffer.set(
            geom.cross as u16,
            screen_y,
            Cell::with_role(ch, fg, bg, CellRole::Scrollbar),
        );
    }
}

/// Render a horizontal scrollbar from the unified model geometry.
///
/// Paints the bar's strip on its dedicated row: arrows at the strip
/// ends when the config shows them, the arrow-free track between.
/// All positions come from [`crate::core::geometry::ScrollbarGeom`];
/// nothing here recomputes them.
pub fn render_horizontal(
    buffer: &mut ScreenBuffer,
    geom: &crate::core::geometry::ScrollbarGeom,
    track_fg: Color,
    thumb_fg: Color,
    track_bg: Color,
) {
    use crate::config::scrollbar::{GLYPHS, SHOW_ARROWS};

    let start = geom.strip_start();
    let len = geom.strip_len();
    if len == 0 {
        return;
    }
    for x in 0..len {
        let screen_x = (start + x) as u16;
        let (ch, fg, bg) = if SHOW_ARROWS && x == 0 {
            (GLYPHS.arrow_left, thumb_fg, track_bg)
        } else if SHOW_ARROWS && x + 1 == len {
            (GLYPHS.arrow_right, thumb_fg, track_bg)
        } else {
            let track_x = x.saturating_sub(usize::from(SHOW_ARROWS));
            if track_x >= geom.thumb_start && track_x < geom.thumb_start + geom.thumb_len {
                (GLYPHS.thumb, thumb_fg, track_bg)
            } else {
                (GLYPHS.track_h, track_fg, track_bg)
            }
        };
        buffer.set(
            screen_x,
            geom.cross as u16,
            Cell::with_role(ch, fg, bg, CellRole::Scrollbar),
        );
    }
}
