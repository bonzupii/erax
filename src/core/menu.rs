//! Menu system: schema, runtime assembly, and derived presentation.
//!
//! Menu STRUCTURE is data: [`MenuSpec`] tables live in
//! [`crate::config::menu`] as consts with per-entry enable/disable, and this
//! module interprets them. Labels and hotkey hints are never stored: the
//! label derives from the command's standard name
//! ([`Command::name`], title-cased by [`display_name`]), and the hint is
//! resolved at render time from the live binding tables for the active
//! input model ([`hint_for`]) and rendered through the canonical notation
//! ([`crate::core::keyfmt`]). A stored label or hint string would drift from
//! the tables it describes; derivation kills that class by construction.

use crate::core::bindings::{InputModel, ModalAction, ModalMode};
use crate::core::command::Command;
use crate::core::input::KeyPress;

/// One menu's structure, as data: the tables in [`crate::config::menu`]
/// are written in this vocabulary.
#[derive(Clone, Copy, Debug)]
pub struct MenuSpec {
    /// Title shown in the menu bar.
    pub title: &'static str,
    /// Entries in display order.
    pub items: &'static [MenuItemSpec],
}

/// One entry of a [`MenuSpec`]: a command or a separator.
///
/// No label, no hint: both derive from the command at render time.
#[derive(Clone, Copy, Debug)]
pub enum MenuItemSpec {
    /// A command entry. Disabled entries do not render; the data remains.
    Action {
        command: Command,
        enabled: bool,
    },
    /// A separator line.
    Separator,
}

/// A single runtime menu item (action or separator).
#[derive(Clone, Debug)]
pub enum MenuItem {
    /// Action item: the typed command and whether it renders.
    Action { command: Command, enabled: bool },
    /// Separator line
    Separator,
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

    /// Whether the item at `idx` can be highlighted and run.
    pub fn selectable(&self, idx: usize) -> bool {
        match self.items.get(idx) {
            Some(MenuItem::Action { enabled, .. }) => *enabled,
            _ => false,
        }
    }

    /// Move selection down.
    ///
    /// If no item is selectable the selection is left unchanged rather
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
            if self.selectable(idx) {
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
    /// If no item is selectable the selection is left unchanged rather
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
            if self.selectable(idx) {
                self.selected = Some(idx);
                return;
            }
            if idx == start {
                return;
            }
        }
    }

    /// The item painted at rendered row `row`.
    ///
    /// Disabled entries are not painted, so rendered rows and item indices
    /// diverge when any entry is disabled; mouse hit-testing must resolve
    /// through this method, never index `items` by screen row.
    pub fn rendered_item(&self, row: usize) -> Option<&MenuItem> {
        self.items
            .iter()
            .filter(|item| {
                !matches!(item, MenuItem::Action { enabled: false, .. })
            })
            .nth(row)
    }

    /// The item index painted at rendered row `row`, for hover selection.
    pub fn rendered_row_index(&self, row: usize) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                !matches!(item, MenuItem::Action { enabled: false, .. })
            })
            .nth(row)
            .map(|(idx, _)| idx)
    }

    /// Get the currently selected item's command (if any)
    pub fn selected_command(&self) -> Option<Command> {
        self.selected.and_then(|idx| match self.items.get(idx) {
            Some(MenuItem::Action {
                command, enabled: true, ..
            }) => Some(*command),
            _ => None,
        })
    }

    /// Render data for all menu items: (label, hotkey hint, is separator).
    ///
    /// Labels derive from the command's standard name; hints resolve from
    /// the live binding tables for `model`. Disabled entries do not render.
    pub fn render_items(&self, model: InputModel) -> Vec<(String, Option<String>, bool)> {
        self.items
            .iter()
            .filter_map(|item| match item {
                MenuItem::Action {
                    command,
                    enabled: true,
                } => Some((
                    display_name(*command),
                    hint_for(model, *command),
                    false,
                )),
                MenuItem::Action { enabled: false, .. } => None,
                MenuItem::Separator => Some(("---".to_string(), None, true)),
            })
            .collect()
    }

    /// Calculate width needed to render this menu (for dropdown sizing)
    pub fn render_width(&self, model: InputModel) -> usize {
        let max_label = self
            .render_items(model)
            .iter()
            .map(|(label, hotkey, is_sep)| {
                if *is_sep {
                    3
                } else {
                    let hotkey_len = hotkey.as_ref().map_or(0, |h| h.len() + 2);
                    label.len() + hotkey_len
                }
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
    /// Assemble the menu bar from the config data tables.
    pub fn from_spec(specs: &[MenuSpec]) -> Self {
        Self {
            menus: specs
                .iter()
                .map(|spec| Menu {
                    title: spec.title,
                    items: spec
                        .items
                        .iter()
                        .map(|item| match item {
                            MenuItemSpec::Action { command, enabled } => MenuItem::Action {
                                command: *command,
                                enabled: *enabled,
                            },
                            MenuItemSpec::Separator => MenuItem::Separator,
                        })
                        .collect(),
                    selected: None,
                })
                .collect(),
            active_menu: None,
        }
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

    /// Execute the selected command (returns the typed command)
    pub fn execute_selected(&mut self) -> Option<Command> {
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
        Self::from_spec(crate::config::menu::MENUS)
    }
}

/// The menu label for a command: its standard kebab-case name, title-cased.
///
/// Glue words stay lowercase (unless first) so labels read like
/// contemporary editor menus: `find-and-replace` becomes "Find and Replace".
/// A pure function of [`Command::name`]: the prompt, the menu, and help all
/// name the command identically by construction.
pub fn display_name(command: Command) -> String {
    const GLUE: &[&str] = &[
        "and", "or", "to", "of", "a", "an", "the", "for", "in", "on", "with", "without", "as",
        "at", "by", "from",
    ];
    command
        .name()
        .split('-')
        .enumerate()
        .map(|(i, word)| {
            if i > 0 && GLUE.contains(&word) {
                word.to_string()
            } else {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The hotkey hint for a command in `model`: the command's binding in that
/// model's live table, rendered through the canonical notation.
///
/// Returns `None` when the command has no binding in the model (facility-
/// reachable commands show no hint rather than a hand-typed string).
pub fn hint_for(model: InputModel, command: Command) -> Option<String> {
    let keys: &[KeyPress] = match model {
        InputModel::KeyTrie | InputModel::Standard => {
            crate::config::keys::bindings_for_model(model)
                .iter()
                .find(|b| b.command == command)?
                .keys
        }
        InputModel::Modal => {
            // Normal-mode direct command bindings only: the `:` facility,
            // `g`-prefixes, and operator compositions have no single chord.
            let binding = crate::config::keys::MODAL_BINDINGS.iter().find(|b| {
                b.mode == ModalMode::Normal
                    && matches!(b.action, ModalAction::Command(c) if c == command)
            })?;
            std::slice::from_ref(&binding.key)
        }
    };
    Some(crate::core::keyfmt::format_sequence(keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::menu::MENUS;

    fn spec_bar() -> MenuBar {
        MenuBar::from_spec(MENUS)
    }

    #[test]
    fn spec_assembles_all_menus() {
        let bar = spec_bar();
        assert_eq!(bar.menus.len(), MENUS.len());
        for (menu, spec) in bar.menus.iter().zip(MENUS.iter()) {
            assert_eq!(menu.title, spec.title);
            assert_eq!(menu.items.len(), spec.items.len());
        }
    }

    #[test]
    fn disabled_entries_do_not_render_but_data_remains() {
        let spec = [MenuSpec {
            title: "t",
            items: &[
                MenuItemSpec::Action {
                    command: Command::Undo,
                    enabled: true,
                },
                MenuItemSpec::Action {
                    command: Command::Redo,
                    enabled: false,
                },
                MenuItemSpec::Separator,
            ],
        }];
        let bar = MenuBar::from_spec(&spec);
        // Data remains: three items assembled.
        assert_eq!(bar.menus[0].items.len(), 3);
        // Rendering skips the disabled entry.
        let items = bar.menus[0].render_items(InputModel::KeyTrie);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|(label, _, _)| label != "Redo"));
    }

    #[test]
    fn selection_skips_separators_and_disabled() {
        let spec = [MenuSpec {
            title: "t",
            items: &[
                MenuItemSpec::Action {
                    command: Command::Undo,
                    enabled: true,
                },
                MenuItemSpec::Separator,
                MenuItemSpec::Action {
                    command: Command::Redo,
                    enabled: false,
                },
                MenuItemSpec::Action {
                    command: Command::DescribeKey,
                    enabled: true,
                },
            ],
        }];
        let mut bar = MenuBar::from_spec(&spec);
        bar.open_menu(0);
        let menu = bar.active().unwrap();
        // Index 0 is Undo; next skips the separator AND the disabled Redo.
        menu.select_next();
        assert_eq!(menu.selected, Some(3));
        menu.select_next();
        assert_eq!(menu.selected, Some(0));
    }

    #[test]
    fn select_prev_skips_separators_and_disabled() {
        let spec = [MenuSpec {
            title: "t",
            items: &[
                MenuItemSpec::Action {
                    command: Command::Undo,
                    enabled: true,
                },
                MenuItemSpec::Separator,
                MenuItemSpec::Action {
                    command: Command::DescribeKey,
                    enabled: true,
                },
            ],
        }];
        let mut bar = MenuBar::from_spec(&spec);
        bar.open_menu(0);
        let menu = bar.active().unwrap();
        menu.select_prev();
        assert_eq!(menu.selected, Some(2));
    }

    /// L-001: a menu with nothing selectable must not hang select_next/prev.
    #[test]
    fn all_unselectable_menu_does_not_hang() {
        let spec = [MenuSpec {
            title: "t",
            items: &[
                MenuItemSpec::Separator,
                MenuItemSpec::Separator,
                MenuItemSpec::Action {
                    command: Command::Undo,
                    enabled: false,
                },
            ],
        }];
        let mut bar = MenuBar::from_spec(&spec);
        bar.open_menu(0);
        let menu = bar.active().unwrap();
        menu.select_next();
        assert_eq!(menu.selected, Some(0));
        menu.select_prev();
        assert_eq!(menu.selected, Some(0));
    }

    #[test]
    fn menu_navigation() {
        let mut bar = spec_bar();
        assert!(!bar.is_open());

        bar.open_menu(0);
        assert!(bar.is_open());
        assert_eq!(bar.active_menu, Some(0));

        bar.next_menu();
        assert_eq!(bar.active_menu, Some(1));

        bar.prev_menu();
        assert_eq!(bar.active_menu, Some(0));

        bar.close();
        assert!(!bar.is_open());
    }

    #[test]
    fn execute_selected_returns_the_typed_command() {
        let mut bar = spec_bar();
        bar.open_menu(0);
        let cmd = bar.execute_selected();
        assert!(cmd.is_some());
        assert!(!bar.is_open());
    }

    #[test]
    fn display_name_title_cases_with_glue_words() {
        assert_eq!(display_name(Command::SaveBuffer), "Save");
        assert_eq!(display_name(Command::QueryReplace), "Find and Replace");
        assert_eq!(display_name(Command::GotoLine), "Go to Line");
        assert_eq!(
            display_name(Command::ExitWithoutSave),
            "Quit without Saving"
        );
        assert_eq!(
            display_name(Command::SplitCurrentWindow),
            "Split Window Down"
        );
        assert_eq!(display_name(Command::Undo), "Undo");
    }

    #[test]
    fn hint_derives_from_the_live_keytrie_table() {
        // SaveBuffer is bound as Ctrl+x Ctrl+s in the KeyTrie table.
        let hint = hint_for(InputModel::KeyTrie, Command::SaveBuffer);
        assert_eq!(hint.as_deref(), Some("Ctrl+x-Ctrl+s"));
    }

    #[test]
    fn hint_is_none_when_the_model_binds_nothing() {
        // GotoByte is facility-only in the KeyTrie model (no binding).
        assert_eq!(hint_for(InputModel::KeyTrie, Command::GotoByte), None);
    }

    #[test]
    fn modal_hint_uses_the_normal_mode_chord() {
        // 'h' runs BackwardCharacter in Normal mode.
        assert_eq!(
            hint_for(InputModel::Modal, Command::BackwardCharacter).as_deref(),
            Some("h")
        );
    }
}
