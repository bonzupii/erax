//! Menubar rendering for TUI mode
//!
//! All menubar logic consolidated here: menu titles, dropdowns, X close button.

use crate::core::menu::MenuBar;
use crate::terminal::display::{Cell, Color, ScreenBuffer};
use crate::terminal::theme::Theme;

/// Renders the menu bar
pub struct MenuRenderer;

impl MenuRenderer {
    /// Render the menu bar to a screen buffer
    pub fn render(
        buffer: &mut ScreenBuffer,
        menu_bar: &MenuBar,
        theme: &Theme,
        width: usize,
        height: usize,
        show: bool,
    ) {
        if !show {
            return;
        }

        // Calculate theme colors
        let crate::terminal::theme::Color::Rgb {
            r: bg_r,
            g: bg_g,
            b: bg_b,
        } = theme.bg().clone();
        let is_light_theme = ((bg_r as u16 + bg_g as u16 + bg_b as u16) / 3) > 128;

        let menu_bg: Color = if is_light_theme {
            theme.gray_dark().clone().into()
        } else {
            Color::Rgb {
                r: bg_r.saturating_sub(15),
                g: bg_g.saturating_sub(15),
                b: bg_b.saturating_sub(10),
            }
        };
        let menu_fg: Color = if is_light_theme {
            theme.bg().clone().into()
        } else {
            theme.fg().clone().into()
        };
        let menu_active_bg: Color = theme.selection_bg().clone().into();
        let menu_active_fg: Color = theme.selection_fg().clone().into();

        // Draw menu bar background
        for x in 0..width {
            buffer.set(x as u16, 0, Cell::new(' ', menu_fg, menu_bg));
        }

        // Draw menu titles
        let layout = menu_bar.layout();
        for (i, (title, start, end)) in layout.iter().enumerate() {
            let is_active = menu_bar.active_menu == Some(i);
            let fg = if is_active { menu_active_fg } else { menu_fg };
            let bg = if is_active { menu_active_bg } else { menu_bg };

            // Padding before title
            buffer.set(*start as u16, 0, Cell::new(' ', fg, bg));

            // Title chars
            for (j, ch) in title.chars().enumerate() {
                let x = start + 1 + j;
                if x < width {
                    buffer.set(x as u16, 0, Cell::new(ch, fg, bg));
                }
            }

            // Padding after title
            if *end < width {
                buffer.set((*end - 1) as u16, 0, Cell::new(' ', fg, bg));
            }
        }

        // Calculate where menu titles end
        let menu_end = layout.last().map(|(_, _, end)| *end).unwrap_or(0);

        // Reserve space for X button (2 chars: space + X)
        let x_button_start = width.saturating_sub(2);

        // Draw horizontal scrollbar indicator between menu and X button
        // This shows a progress bar for horizontal scroll position
        let scrollbar_start = menu_end + 1;
        let scrollbar_end = x_button_start.saturating_sub(1);
        let scrollbar_width = scrollbar_end.saturating_sub(scrollbar_start);

        if scrollbar_width >= 3 {
            let scrollbar_fg: Color = theme.gray_light().clone().into();
            // Draw scrollbar track
            for x in scrollbar_start..scrollbar_end {
                buffer.set(x as u16, 0, Cell::new('─', scrollbar_fg, menu_bg));
            }
        }

        // Draw X close button at far right with padding
        if width > 3 {
            let x_fg: Color = theme.error().clone().into();
            buffer.set((width - 2) as u16, 0, Cell::new(' ', menu_fg, menu_bg)); // padding
            buffer.set((width - 1) as u16, 0, Cell::new('✕', x_fg, menu_bg));
        }

        // Draw dropdown menu if open
        if let Some(active_idx) = menu_bar.active_menu {
            let menu = &menu_bar.menus[active_idx];
            let items = menu.render_items();
            let menu_width = menu.render_width();

            if let Some((_, start_x, _)) = layout.get(active_idx) {
                for (item_idx, (label, hotkey, is_sep, _is_submenu)) in items.iter().enumerate() {
                    let y = 1 + item_idx;
                    if y >= height {
                        break;
                    }

                    let is_selected = menu.selected == Some(item_idx);
                    let fg = if is_selected { menu_active_fg } else { menu_fg };
                    let bg = if is_selected { menu_active_bg } else { menu_bg };

                    // Clear row
                    for x in *start_x..(*start_x + menu_width).min(width) {
                        buffer.set(x as u16, y as u16, Cell::new(' ', fg, bg));
                    }

                    if *is_sep {
                        for x in *start_x..(*start_x + menu_width).min(width) {
                            buffer.set(x as u16, y as u16, Cell::new('─', fg, bg));
                        }
                    } else {
                        // Label
                        for (j, ch) in label.chars().enumerate() {
                            let x = start_x + 1 + j;
                            if x < width && x < start_x + menu_width {
                                buffer.set(x as u16, y as u16, Cell::new(ch, fg, bg));
                            }
                        }

                        // Hotkey
                        if let Some(hk) = hotkey {
                            let hk_start = start_x + menu_width - hk.len() - 2;
                            for (j, ch) in hk.chars().enumerate() {
                                let x = hk_start + j;
                                if x < width && x >= *start_x {
                                    buffer.set(x as u16, y as u16, Cell::new(ch, fg, bg));
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
