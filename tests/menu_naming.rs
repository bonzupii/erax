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
/// (Exhaustive over the inventory: a new command is covered automatically;
/// the menu-coverage test below forces its menu entry.)
#[test]
fn every_command_has_exactly_one_standard_name() {
    let all = Command::all();
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

/// Display names derive from the standard names; the derivation is total
/// and collision-free (two commands must never share a menu label).
#[test]
fn display_names_derive_from_standard_names() {
    let mut seen = HashSet::new();
    for cmd in Command::all() {
        let label = display_name(*cmd);
        assert!(!label.is_empty(), "{cmd:?} has an empty display name");
        // First letter capitalized, no kebab separators survive.
        assert!(
            label.chars().next().unwrap().is_uppercase(),
            "label not capitalized: {label}"
        );
        assert!(!label.contains('-'), "kebab separator in label: {label}");
        assert!(
            seen.insert(label.clone()),
            "display-name collision on {label:?}"
        );
    }
}

/// Hints derive from the live tables: for every command a model's table
/// binds, the hint is the canonical rendering of the table's own entry —
/// not a pinned string, so rebinding the table moves the hint with it.
/// A command with no table entry shows no hint, never a hand-typed string.
#[test]
fn hints_derive_from_live_tables_per_model() {
    use erax::config::keys::{bindings_for_model, MODAL_BINDINGS};
    use erax::core::bindings::{ModalAction, ModalMode};
    use erax::core::keyfmt::format_sequence;

    for model in [InputModel::KeyTrie, InputModel::Standard] {
        let mut seen = HashSet::new();
        for binding in bindings_for_model(model) {
            // First table entry wins, mirroring hint_for.
            if !seen.insert(binding.command) {
                continue;
            }
            assert_eq!(
                hint_for(model, binding.command).as_deref(),
                Some(format_sequence(binding.keys)).as_deref(),
                "{model:?}: hint must render the table's binding for {:?}",
                binding.command
            );
        }
    }
    // Modal: Normal-mode direct command chords only.
    let mut seen = HashSet::new();
    for binding in MODAL_BINDINGS {
        if binding.mode != ModalMode::Normal {
            continue;
        }
        if let ModalAction::Command(cmd) = binding.action
            && seen.insert(cmd)
        {
            let keys = [binding.key];
            assert_eq!(
                hint_for(InputModel::Modal, cmd).as_deref(),
                Some(format_sequence(&keys)).as_deref(),
                "modal hint must render the Normal-mode chord for {cmd:?}"
            );
        }
    }
    // Facility-only commands show no hint.
    assert_eq!(hint_for(InputModel::KeyTrie, Command::GotoByte), None);
    assert_eq!(hint_for(InputModel::Standard, Command::WrapWord), None);
    assert_eq!(hint_for(InputModel::Modal, Command::GotoByte), None);
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
