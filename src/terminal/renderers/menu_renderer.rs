//! Menubar rendering for TUI mode.
//!
//! All menubar logic consolidated here: menu titles, dropdowns, X close button.

use crate::core::bindings::InputModel;
use crate::core::menu::MenuBar;
use crate::terminal::display::{Cell, CellRole, Color, ScreenBuffer};
use crate::terminal::theme::Theme;

/// Renders the menu bar
pub struct MenuRenderer;

impl MenuRenderer {
    /// Render the menu bar to a screen buffer.
    ///
    /// `model` is the active input model: hotkey hints resolve from its
    /// live binding tables, so a Vim-feel session never shows Emacs chords.
    pub fn render(
        buffer: &mut ScreenBuffer,
        menu_bar: &MenuBar,
        theme: &Theme,
        width: usize,
        height: usize,
        show: bool,
        model: InputModel,
    ) {
        if !show {
            return;
        }

        // Calculate theme colors
        let (bg_r, bg_g, bg_b) = theme.bg.rgb8();
        let is_light_theme = ((bg_r as u16 + bg_g as u16 + bg_b as u16) / 3) > 128;

        let menu_bg: Color = if is_light_theme {
            theme.comment
        } else {
            Color::Rgb {
                r: bg_r.saturating_sub(15),
                g: bg_g.saturating_sub(15),
                b: bg_b.saturating_sub(10),
            }
        };
        let menu_fg: Color = if is_light_theme {
            theme.bg
        } else {
            theme.fg
        };
        let menu_active_bg: Color = theme.selection_bg;
        let menu_active_fg: Color = theme.selection_fg;

        // Draw menu bar background
        for x in 0..width {
            buffer.set(x as u16, 0, Cell::with_role(' ', menu_fg, menu_bg, CellRole::MenuBar));
        }

        // Draw menu titles
        let layout = menu_bar.layout();
        for (i, (title, start, end)) in layout.iter().enumerate() {
            let is_active = menu_bar.active_menu == Some(i);
            let fg = if is_active { menu_active_fg } else { menu_fg };
            let bg = if is_active { menu_active_bg } else { menu_bg };

            // Padding before title
            buffer.set(*start as u16, 0, Cell::with_role(' ', fg, bg, CellRole::MenuBar));

            // Title chars
            for (j, ch) in title.chars().enumerate() {
                let x = start + 1 + j;
                if x < width {
                    buffer.set(x as u16, 0, Cell::with_role(ch, fg, bg, CellRole::MenuBar));
                }
            }

            // Padding after title
            if *end < width {
                buffer.set((*end - 1) as u16, 0, Cell::with_role(' ', fg, bg, CellRole::MenuBar));
            }
        }

        // Draw dropdown menu if open
        if let Some(active_idx) = menu_bar.active_menu {
            let menu = &menu_bar.menus[active_idx];
            let items = menu.render_items(model);
            let menu_width = menu.render_width(model);

            if let Some((_, start_x, _)) = layout.get(active_idx) {
                for (item_idx, (label, hotkey, is_sep)) in items.iter().enumerate() {
                    let y = 1 + item_idx;
                    if y >= height {
                        break;
                    }

                    let is_selected = menu.selected == Some(item_idx);
                    let fg = if is_selected { menu_active_fg } else { menu_fg };
                    let bg = if is_selected { menu_active_bg } else { menu_bg };

                    // Clear row
                    for x in *start_x..(*start_x + menu_width).min(width) {
                        buffer.set(x as u16, y as u16, Cell::with_role(' ', fg, bg, CellRole::MenuItem));
                    }

                    if *is_sep {
                        for x in *start_x..(*start_x + menu_width).min(width) {
                            buffer.set(x as u16, y as u16, Cell::with_role('─', fg, bg, CellRole::MenuItem));
                        }
                    } else {
                        // Label
                        for (j, ch) in label.chars().enumerate() {
                            let x = start_x + 1 + j;
                            if x < width && x < start_x + menu_width {
                                buffer.set(x as u16, y as u16, Cell::with_role(ch, fg, bg, CellRole::MenuItem));
                            }
                        }

                        // Hotkey
                        if let Some(hk) = hotkey {
                            let hk_start = start_x + menu_width - hk.len() - 2;
                            for (j, ch) in hk.chars().enumerate() {
                                let x = hk_start + j;
                                if x < width && x >= *start_x {
                                    buffer.set(x as u16, y as u16, Cell::with_role(ch, fg, bg, CellRole::MenuItem));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
