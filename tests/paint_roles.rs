//! Paint-path role stamps: gutter chrome and theme resolution through the real render path.
//!
//! The presentation core consumes cell roles (never glyphs or colors) to
//! decide what a cell is, so the TUI painters must stamp chrome roles
//! honestly: diagnostic icons are Diagnostic markers, tilde filler rows
//! are gutter chrome, and the backend's resolved theme (invocation
//! overrides included) is the theme the cells are painted with.

use erax::core::app::EditorApp;
use erax::core::diagnostics::{Diagnostic, DiagnosticSeverity};
use erax::terminal::display::{CellRole, Display};
use erax::terminal::theme::ThemeId;

fn rendered_app(text: &str) -> (EditorApp, Display) {
    let mut app = EditorApp::new();
    app.active_buffer_mut().unwrap().insert(0, text);
    let mut display = Display::new(80, 24, None, None);
    display.render(&mut app).expect("render");
    (app, display)
}

#[test]
fn diagnostic_icon_carries_diagnostic_role_at_gutter_edge() {
    let mut app = EditorApp::new();
    app.active_buffer_mut().unwrap().insert(0, "one\ntwo\nthree\n");
    app.active_buffer_mut()
        .unwrap()
        .add_diagnostic(Diagnostic::new(
            DiagnosticSeverity::Error,
            "main.rs",
            2,
            "boom",
        ));
    let mut display = Display::new(80, 24, None, None);
    display.render(&mut app).expect("render");

    // Buffer line 2 paints on the second text row (row 0 is the menu bar).
    let row = 2u16;
    // Find the icon: the error marker in the gutter columns.
    let mut icon_x = None;
    for x in 0..8u16 {
        let cell = display.back_buffer.get(x, row).unwrap();
        if cell.text() == "!" {
            icon_x = Some(x);
            assert_eq!(
                cell.role,
                CellRole::Diagnostic,
                "diagnostic icon must carry the Diagnostic role, not EditorText"
            );
        }
    }
    let icon_x = icon_x.expect("error diagnostic must paint an icon in the gutter");
    // The icon sits at the gutter's right edge — the divider's seat —
    // derived from the same origin as the line numbers: the cell to its
    // left is a line-number gutter cell, never a digit the icon clobbered
    // from a misaligned origin.
    let left = display.back_buffer.get(icon_x - 1, row).unwrap();
    assert_eq!(
        left.role,
        CellRole::Gutter,
        "cell left of the icon must be a gutter cell"
    );
}

#[test]
fn tilde_filler_row_carries_gutter_role() {
    let (_app, display) = rendered_app("only\n");
    // Row 0 is the menu bar, row 1 holds the text; row 5 is filler.
    let row = 5u16;
    let tilde = display
        .back_buffer
        .cells
        .iter()
        .skip(row as usize * 80)
        .take(80)
        .find(|c| c.text() == "~")
        .expect("filler row must carry a tilde");
    assert_eq!(
        tilde.role,
        CellRole::Gutter,
        "tilde filler is gutter chrome, not document text"
    );
    // The whole filler row is chrome: every cell carries the Gutter role.
    for x in 0..80u16 {
        let cell = display.back_buffer.get(x, row).unwrap();
        assert_eq!(
            cell.role,
            CellRole::Gutter,
            "filler row cell ({x}, {row}) must be gutter chrome"
        );
    }
}

#[test]
fn display_honors_theme_override() {
    // The invocation override shadows the config default without mutating
    // it; the resolved theme is what the cells are painted with, and what
    // the presentation core must render from.
    let over = Display::new(80, 24, Some(ThemeId::Dracula), None);
    assert_eq!(
        over.theme.bg,
        ThemeId::Dracula.theme().bg,
        "theme override must resolve to the override theme"
    );
    let plain = Display::new(80, 24, None, None);
    assert_eq!(
        plain.theme.bg,
        erax::config::theme::THEME.theme().bg,
        "no override must resolve to the config default"
    );
}
