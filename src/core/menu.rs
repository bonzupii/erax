//! Menu system data model.
//!
//! Menu data model used by both TUI and GUI modes.
//! Rendering is handled by terminal/renderers/menu_renderer.rs.

/// Height of the menu bar in cells
pub const MENU_BAR_HEIGHT_CELLS: usize = 1;

/// A single menu item (action or separator)
#[derive(Clone, Debug)]
pub enum MenuItem {
    /// Action item with label, typed command, and optional hotkey hint
    Action {
        label: &'static str,
        command: crate::core::command::Command,
        hotkey: Option<&'static str>,
    },
    /// Separator line
    Separator,
}

impl MenuItem {
    /// Create an action item
    pub const fn action(
        label: &'static str,
        command: crate::core::command::Command,
        hotkey: Option<&'static str>,
    ) -> Self {
        MenuItem::Action {
            label,
            command,
            hotkey,
        }
    }

    /// Create a separator
    pub const fn separator() -> Self {
        MenuItem::Separator
    }
}

/// A dropdown menu (column of MenuItems)
#[derive(Clone, Debug)]
pub struct Menu {
    /// Menu title (shown in menu bar)
    pub title: &'static str,
    /// Items in this menu
    pub items: Vec<MenuItem>,
    /// Currently highlighted item index (None if menu closed)
    pub selected: Option<usize>,
}

impl Menu {
    /// Create a menu with title and items
    pub fn new(title: &'static str, items: Vec<MenuItem>) -> Self {
        Self {
            title,
            items,
            selected: None,
        }
    }

    /// Move selection down.
    ///
    /// If every item is a separator the selection is left unchanged rather
    /// than spinning forever (L-001).
    pub fn select_next(&mut self) {
        let max = self.items.len();
        if max == 0 {
            return;
        }

        let start = self.selected.unwrap_or(0) % max;
        let mut idx = start;
        loop {
            idx = (idx + 1) % max;
            if !matches!(self.items.get(idx), Some(MenuItem::Separator)) {
                self.selected = Some(idx);
                return;
            }
            if idx == start {
                return;
            }
        }
    }

    /// Move selection up.
    ///
    /// If every item is a separator the selection is left unchanged rather
    /// than spinning forever (L-001).
    pub fn select_prev(&mut self) {
        let max = self.items.len();
        if max == 0 {
            return;
        }

        let start = self.selected.unwrap_or(0) % max;
        let mut idx = start;
        loop {
            idx = if idx == 0 { max - 1 } else { idx - 1 };
            if !matches!(self.items.get(idx), Some(MenuItem::Separator)) {
                self.selected = Some(idx);
                return;
            }
            if idx == start {
                return;
            }
        }
    }

    /// Get the currently selected item's command (if any)
    pub fn selected_command(&self) -> Option<crate::core::command::Command> {
        let result = self.selected.and_then(|idx| {
            #[cfg(debug_assertions)]
            eprintln!(
                "selected_command: idx={}, item={:?}",
                idx,
                self.items.get(idx)
            );
            match self.items.get(idx) {
                Some(MenuItem::Action { command, .. }) => Some(*command),
                _ => None,
            }
        });
        #[cfg(debug_assertions)]
        eprintln!("selected_command returning: {:?}", result);
        result
    }

    /// Get render data for all menu items
    /// Returns vec of (label, hotkey_hint, is_separator, is_submenu)
    pub fn render_items(&self) -> Vec<(String, Option<&'static str>, bool, bool)> {
        self.items
            .iter()
            .map(|item| match item {
                MenuItem::Action { label, hotkey, .. } => {
                    (label.to_string(), *hotkey, false, false)
                }
                MenuItem::Separator => ("---".to_string(), None, true, false),
            })
            .collect()
    }

    /// Calculate width needed to render this menu (for dropdown sizing)
    pub fn render_width(&self) -> usize {
        let max_label = self
            .items
            .iter()
            .map(|item| match item {
                MenuItem::Action { label, hotkey, .. } => {
                    let hotkey_len = hotkey.map_or(0, |h| h.len() + 2);
                    label.len() + hotkey_len
                }
                MenuItem::Separator => 3,
            })
            .max();

        let max_label = max_label.unwrap_or(10);

        max_label + 4 // padding
    }
}

/// The menu bar containing all top-level menus
#[derive(Clone, Debug)]
pub struct MenuBar {
    /// All menus in the bar
    pub menus: Vec<Menu>,
    /// Currently open menu index (None if all closed)
    pub active_menu: Option<usize>,
}

impl MenuBar {
    /// Create the standard erax menu bar
    pub fn new() -> Self {
        Self {
            menus: vec![
                Self::file_menu(),
                Self::edit_menu(),
                Self::view_menu(),
                Self::buffer_menu(),
                Self::window_menu(),
                Self::help_menu(),
            ],
            active_menu: None,
        }
    }

    fn file_menu() -> Menu {
        Menu::new(
            "File",
            vec![
                MenuItem::action("Find File...", crate::core::command::Command::FindFile, Some("^X^F")),
                MenuItem::action("Read File...", crate::core::command::Command::ReadFile, Some("^X^R")),
                MenuItem::separator(),
                MenuItem::action("Save", crate::core::command::Command::SaveBuffer, Some("^X^S")),
                MenuItem::action("Write File...", crate::core::command::Command::WriteFile, Some("^X^W")),
                MenuItem::separator(),
                MenuItem::action("Print Buffer", crate::core::command::Command::PrintBuffer, None),
                MenuItem::separator(),
                MenuItem::action("Exit", crate::core::command::Command::ExitErax, Some("^X^C")),
            ],
        )
    }

    fn edit_menu() -> Menu {
        Menu::new(
            "Edit",
            vec![
                MenuItem::action("Set Mark", crate::core::command::Command::SetMark, Some("M-Space")),
                MenuItem::action(
                    "Exchange Point/Mark",
                    crate::core::command::Command::ExchangePointAndMark,
                    Some("^X^X"),
                ),
                MenuItem::separator(),
                MenuItem::action("Kill Region", crate::core::command::Command::KillRegion, Some("^W")),
                MenuItem::action("Copy Region", crate::core::command::Command::CopyRegion, Some("M-W")),
                MenuItem::action("Yank", crate::core::command::Command::Yank, Some("^Y")),
                MenuItem::separator(),
                MenuItem::action("Kill Line", crate::core::command::Command::KillToEndOfLine, Some("^K")),
                MenuItem::action("Delete Blank Lines", crate::core::command::Command::DeleteBlankLines, Some("^X^O")),
                MenuItem::separator(),
                MenuItem::action("Search Forward", crate::core::command::Command::SearchForward, Some("^S")),
                MenuItem::action("Search Backward", crate::core::command::Command::SearchReverse, Some("^R")),
                MenuItem::action("Query Replace", crate::core::command::Command::QueryReplace, Some("M-^R")),
            ],
        )
    }

    fn view_menu() -> Menu {
        Menu::new(
            "View",
            vec![
                MenuItem::action("Toggle Diagnostics", crate::core::command::Command::ToggleDiagnostics, None),
                MenuItem::action("Jump to Diagnostic", crate::core::command::Command::DiagnosticsJump, None),
                MenuItem::separator(),
                MenuItem::action("Buffer Info", crate::core::command::Command::BufferInfo, None),
                MenuItem::action("What Cursor Position", crate::core::command::Command::WhatCursorPosition, Some("^X =")),
            ],
        )
    }

    fn buffer_menu() -> Menu {
        Menu::new(
            "Buf",
            vec![
                MenuItem::action("Next Buffer", crate::core::command::Command::NextBuffer, Some("^X X")),
                MenuItem::action("Previous Buffer", crate::core::command::Command::PreviousBuffer, None),
                MenuItem::action("Select Buffer...", crate::core::command::Command::SelectBuffer, Some("^X B")),
                MenuItem::action("List Buffers", crate::core::command::Command::ListBuffers, Some("^X^B")),
                MenuItem::separator(),
                MenuItem::action("Kill Buffer", crate::core::command::Command::DeleteBuffer, Some("^X K")),
            ],
        )
    }

    fn window_menu() -> Menu {
        Menu::new(
            "Win",
            vec![
                MenuItem::action("Split Vertical", crate::core::command::Command::SplitCurrentWindow, Some("^X 2")),
                MenuItem::action(
                    "Split Horizontal",
                    crate::core::command::Command::SplitWindowHorizontally,
                    Some("^X 3"),
                ),
                MenuItem::separator(),
                MenuItem::action("Delete Window", crate::core::command::Command::DeleteWindow, None),
                MenuItem::action("Delete Other Windows", crate::core::command::Command::DeleteOtherWindows, Some("^X 1")),
                MenuItem::action("Minimize Window", crate::core::command::Command::MinimizeWindow, Some("^X 0")),
                MenuItem::action("Window Picker", crate::core::command::Command::WindowPicker, Some("^X 9")),
                MenuItem::separator(),
                MenuItem::action("Next Window", crate::core::command::Command::NextWindow, Some("^X O")),
                MenuItem::separator(),
                MenuItem::action("Grow Window", crate::core::command::Command::GrowWindow, Some("^X Z")),
                MenuItem::action("Shrink Window", crate::core::command::Command::ShrinkWindow, Some("^X^Z")),
            ],
        )
    }

    fn help_menu() -> Menu {
        Menu::new(
            "Help",
            vec![
                MenuItem::action("Describe Key...", crate::core::command::Command::DescribeKey, Some("^X ?")),
                MenuItem::action("Count Words", crate::core::command::Command::CountWords, None),
                MenuItem::action("Show Position", crate::core::command::Command::ShowPosition, None),
            ],
        )
    }

    /// Open a menu by index
    pub fn open_menu(&mut self, index: usize) {
        if index < self.menus.len() {
            // Close any currently open menu
            if let Some(old_idx) = self.active_menu {
                self.menus[old_idx].selected = None;
            }
            self.active_menu = Some(index);
            self.menus[index].selected = Some(0);
        }
    }

    /// Close all menus
    pub fn close(&mut self) {
        if let Some(idx) = self.active_menu {
            self.menus[idx].selected = None;
        }
        self.active_menu = None;
    }

    /// Move to next menu (right)
    pub fn next_menu(&mut self) {
        if let Some(idx) = self.active_menu {
            let next = (idx + 1) % self.menus.len();
            self.open_menu(next);
        }
    }

    /// Move to previous menu (left)
    pub fn prev_menu(&mut self) {
        if let Some(idx) = self.active_menu {
            let prev = if idx == 0 {
                self.menus.len() - 1
            } else {
                idx - 1
            };
            self.open_menu(prev);
        }
    }

    /// Get the active menu (if any)
    pub fn active(&mut self) -> Option<&mut Menu> {
        self.active_menu.map(|idx| &mut self.menus[idx])
    }

    /// Execute the selected command (returns command name)
    pub fn execute_selected(&mut self) -> Option<crate::core::command::Command> {
        let cmd = self
            .active_menu
            .and_then(|idx| self.menus[idx].selected_command());
        self.close();
        cmd
    }

    /// Check if any menu is open
    pub fn is_open(&self) -> bool {
        self.active_menu.is_some()
    }

    /// Get menu bar layout (title, x_start, x_end) for rendering
    pub fn layout(&self) -> Vec<(&'static str, usize, usize)> {
        let mut result = Vec::new();
        let mut x = 1; // Start at column 1 (leave margin)
        for menu in &self.menus {
            let title_len = menu.title.len() + 2; // Add padding
            result.push((menu.title, x, x + title_len));
            x += title_len + 1; // Gap between menus
        }
        result
    }
}

impl Default for MenuBar {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select_next_skips_separators() {
        let mut menu = Menu::new(
            "t",
            vec![
                MenuItem::action("a", crate::core::command::Command::Undo, None),
                MenuItem::Separator,
                MenuItem::action("b", crate::core::command::Command::Redo, None),
            ],
        );
        menu.select_next();
        assert_eq!(menu.selected, Some(2));
        menu.select_next();
        assert_eq!(menu.selected, Some(0));
    }

    #[test]
    fn test_select_prev_skips_separators() {
        let mut menu = Menu::new(
            "t",
            vec![
                MenuItem::action("a", crate::core::command::Command::Undo, None),
                MenuItem::Separator,
                MenuItem::action("b", crate::core::command::Command::Redo, None),
            ],
        );
        menu.select_prev();
        assert_eq!(menu.selected, Some(2));
    }

    /// L-001: an all-separator menu must not hang `select_next`/`select_prev`.
    #[test]
    fn test_all_separator_menu_does_not_hang() {
        let mut menu = Menu::new(
            "t",
            vec![MenuItem::Separator, MenuItem::Separator, MenuItem::Separator],
        );
        menu.select_next();
        assert_eq!(menu.selected, None);
        menu.select_prev();
        assert_eq!(menu.selected, None);
    }

    #[test]
    fn test_menu_navigation() {
        let mut bar = MenuBar::new();
        assert!(!bar.is_open());

        bar.open_menu(0); // Open File menu
        assert!(bar.is_open());
        assert_eq!(bar.active_menu, Some(0));

        bar.next_menu();
        assert_eq!(bar.active_menu, Some(1)); // Edit

        bar.prev_menu();
        assert_eq!(bar.active_menu, Some(0)); // File

        bar.close();
        assert!(!bar.is_open());
    }

    #[test]
    fn test_menu_item_selection() {
        let mut bar = MenuBar::new();
        bar.open_menu(0);

        if let Some(menu) = bar.active() {
            assert_eq!(menu.selected, Some(0));
            menu.select_next();
            if let Some(sel) = menu.selected {
                assert!(sel > 0);
            } else {
                panic!("Expected selected index");
            }
        }
    }

    #[test]
    fn test_execute_selected() {
        let mut bar = MenuBar::new();
        bar.open_menu(0); // File menu

        let cmd = bar.execute_selected();
        assert!(cmd.is_some());
        assert!(!bar.is_open()); // Should close after execute
    }
}
