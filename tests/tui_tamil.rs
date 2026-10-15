//! Tamil grapheme-cluster rendering in the TUI.
//!
//! The window renderer used to iterate per `char` with per-char widths:
//! the Tamil vowel sign U+0BBF (Mc, width 1 per `unicode_width`) was
//! emitted as its own cell while the width model counted one cell per
//! cluster, so cursor, selection, and mouse math disagreed with the
//! emitted cells by one cell per combining mark (and the Mn virama was
//! dropped outright). The renderer now emits one cell per grapheme
//! cluster holding the full cluster, matching the width model and the
//! bytes `cat` would print.

use erax::core::app::EditorApp;
use erax::terminal::display::Display;

/// Collect the text row `y` of the back buffer as a Vec of cell texts.
fn row_texts(display: &Display, y: u16, width: u16) -> Vec<String> {
    (0..width)
        .map(|x| {
            display
                .back_buffer
                .get(x, y)
                .map(|c| c.text().to_string())
                .unwrap_or_default()
        })
        .collect()
}

/// Find consecutive cells holding `expected` anywhere in the back buffer.
/// Returns the (x, y) of the first cell.
fn find_cells(display: &Display, width: u16, height: u16, expected: &[&str]) -> Option<(u16, u16)> {
    for y in 0..height {
        let row = row_texts(display, y, width);
        if let Some(x) = row
            .windows(expected.len())
            .position(|w| w.iter().map(String::as_str).eq(expected.iter().copied()))
        {
            return Some((x as u16, y));
        }
    }
    None
}

#[test]
fn tamil_line_renders_one_cell_per_cluster() {
    let mut app = EditorApp::new();
    app.active_buffer_mut()
        .expect("test app has a buffer")
        .insert(0, "தமிழ்\n");

    let mut display = Display::new(40, 10, None, None);
    display.render(&mut app).expect("render succeeds");

    // "தமிழ்" = த | மி (MA + vowel sign) | ழ் (LLLA + virama).
    let expected = ["த", "மி", "ழ்"];
    let (x, y) =
        find_cells(&display, 40, 10, &expected).expect("Tamil clusters render as consecutive cells");
    let row = row_texts(&display, y, 40);
    // The cells hold the full clusters: no lone combining mark anywhere.
    for (i, cluster) in expected.iter().enumerate() {
        assert_eq!(&row[x as usize + i], cluster);
    }
    // Exactly three cells consumed for three clusters (no extra cell for
    // the vowel sign, no dropped virama).
    let after = &row[x as usize + expected.len()];
    assert!(
        after.trim().is_empty(),
        "no spillover cell after the Tamil line, found {after:?}"
    );
}

#[test]
fn tamil_word_with_vowel_signs_renders_as_clusters() {
    let mut app = EditorApp::new();
    app.active_buffer_mut()
        .expect("test app has a buffer")
        .insert(0, "மொழி\n");

    let mut display = Display::new(40, 10, None, None);
    display.render(&mut app).expect("render succeeds");

    // "மொழி" = மொ (MA + vowel sign O) | ழி (LLLA + vowel sign I).
    // Per-char rendering would emit four cells (ம, ொ, ழ, ி); the width
    // model and the renderer agree on two.
    let expected = ["மொ", "ழி"];
    let (x, y) =
        find_cells(&display, 40, 10, &expected).expect("Tamil word renders as two cluster cells");
    let row = row_texts(&display, y, 40);
    for (i, cluster) in expected.iter().enumerate() {
        assert_eq!(&row[x as usize + i], cluster);
    }
}
