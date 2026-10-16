//! Guards for the menu, naming, and notation invariants.
//!
//! The product surface speaks one standard, product-native command
//! vocabulary: [`Command::name`] is the single source, menu labels derive
//! from it, menu hints derive from the live binding tables, and every
//! binding string on every surface renders through the canonical notation
//! (`crate::core::keyfmt`). These tests pin the derivation chains and the
//! coverage accounting; a stored label or hint string is a defect.

use erax::core::bindings::InputModel;
use erax::core::command::Command;
use erax::core::menu::{display_name, hint_for, MenuBar};
use erax::config::menu::MENUS;
use std::collections::{HashMap, HashSet};

/// Every command has exactly one standard name, and the names are unique.
#[test]
fn every_command_has_exactly_one_standard_name() {
    let all = Command::all();
    assert_eq!(all.len(), 121, "inventory size changed; update this guard");
    let mut seen = HashSet::new();
    for cmd in all {
        assert!(!cmd.name().is_empty(), "{cmd:?} has an empty name");
        assert!(
            seen.insert(cmd.name()),
            "duplicate standard name: {}",
            cmd.name()
        );
        // The name is kebab-case: lowercase words, no model vocabulary.
        assert!(
            cmd.name()
                .chars()
                .all(|c| c.is_ascii_lowercase() || c == '-'),
            "name is not kebab-case: {}",
            cmd.name()
        );
        assert!(
            !cmd.name().contains("emacs") && !cmd.name().contains("vim"),
            "model vocabulary leaked into a standard name: {}",
            cmd.name()
        );
    }
}

/// Menu data covers all commands except the documented exclusions.
#[test]
fn menu_coverage_is_exact() {
    let mut covered: HashMap<Command, usize> = HashMap::new();
    for spec in MENUS {
        for item in spec.items {
            if let erax::core::menu::MenuItemSpec::Action { command, .. } = item {
                *covered.entry(*command).or_insert(0) += 1;
            }
        }
    }
    // Exactly one exclusion, with its reason in the config module docs:
    // PrefixArgument is a prefix mechanism, not an invocable menu action.
    let excluded: HashSet<Command> = [Command::UniversalArgument].into_iter().collect();
    for cmd in Command::all() {
        if excluded.contains(cmd) {
            assert!(
                !covered.contains_key(cmd),
                "{cmd:?} is excluded but appears in a menu"
            );
        } else {
            assert_eq!(
                covered.get(cmd),
                Some(&1),
                "{cmd:?} must appear in exactly one menu"
            );
        }
    }
    assert_eq!(
        covered.len(),
        Command::all().len() - excluded.len(),
        "coverage accounting drifted"
    );
}

/// Display names derive from the standard names; the derivation is total.
#[test]
fn display_names_derive_from_standard_names() {
    for cmd in Command::all() {
        let label = display_name(*cmd);
        assert!(!label.is_empty(), "{cmd:?} has an empty display name");
        // First letter capitalized, no kebab separators survive.
        assert!(
            label.chars().next().unwrap().is_uppercase(),
            "label not capitalized: {label}"
        );
        assert!(!label.contains('-'), "kebab separator in label: {label}");
    }
}

/// Hints derive from the live tables: a bound command shows its binding in
/// the canonical notation; a facility-only command shows no hint.
#[test]
fn hints_derive_from_live_tables_per_model() {
    // KeyTrie: save is Ctrl+x Ctrl+s in the table.
    assert_eq!(
        hint_for(InputModel::KeyTrie, Command::SaveBuffer).as_deref(),
        Some("Ctrl+x-Ctrl+s")
    );
    // Standard: save is Ctrl+s in the table.
    assert_eq!(
        hint_for(InputModel::Standard, Command::SaveBuffer).as_deref(),
        Some("Ctrl+s")
    );
    // Modal: Normal-mode 'h' runs cursor-left.
    assert_eq!(
        hint_for(InputModel::Modal, Command::BackwardCharacter).as_deref(),
        Some("h")
    );
    // Facility-only commands show no hint, never a hand-typed string.
    assert_eq!(hint_for(InputModel::KeyTrie, Command::GotoByte), None);
    assert_eq!(hint_for(InputModel::Standard, Command::WrapWord), None);
}

/// The notation grammar: `+` within a chord, `-` between steps, Alt not M.
#[test]
fn canonical_notation_grammar() {
    use erax::core::input::{Key, KeyPress};
    use erax::core::keyfmt::{format_chord, format_sequence};

    assert_eq!(format_chord(&KeyPress::ctrl(Key::Char('x'))), "Ctrl+x");
    assert_eq!(
        format_chord(&KeyPress::mods(Key::Char('z'), true, false, true)),
        "Ctrl+Shift+z"
    );
    assert_eq!(format_chord(&KeyPress::alt(Key::Char('x'))), "Alt+x");
    assert_eq!(
        format_sequence(&[
            KeyPress::ctrl(Key::Char('x')),
            KeyPress::ctrl(Key::Char('c'))
        ]),
        "Ctrl+x-Ctrl+c"
    );
    assert_eq!(
        format_sequence(&[
            KeyPress::ctrl(Key::Char('x')),
            KeyPress::plain(Key::Char('u'))
        ]),
        "Ctrl+x-u"
    );
}

/// Menu data carries no stored hint or label fields: hints are computed,
/// labels derive, neither is data. (Source-scan guard in the style of
/// tests/bindings_data.rs; it matches field declarations, so prose about
/// the derivation design does not trip it.)
#[test]
fn menu_data_carries_no_stored_hints_or_labels() {
    let src = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/config/menu.rs"
    ))
    .expect("menu config readable");
    for token in ["hotkey:", "hint:", "accelerator:", "label:"] {
        assert!(
            !src.to_lowercase().contains(token),
            "src/config/menu.rs declares a '{token}' field: hints and labels derive, they are not data"
        );
    }
}

/// A disabled entry renders nothing; the command stays reachable by name.
#[test]
fn disabled_entry_renders_nothing_but_stays_reachable() {
    use erax::core::menu::{MenuItemSpec, MenuSpec};
    let spec = [MenuSpec {
        title: "t",
        items: &[MenuItemSpec::Action {
            command: Command::Undo,
            enabled: false,
        }],
    }];
    let bar = MenuBar::from_spec(&spec);
    let rendered: Vec<_> = bar.menus[0].render_items(InputModel::KeyTrie);
    assert!(
        rendered.is_empty(),
        "disabled entry must not render: {rendered:?}"
    );
    // Reachability is by the typed command, unaffected by menu data.
    assert_eq!(Command::from_name(Command::Undo.name()), Some(Command::Undo));
}
