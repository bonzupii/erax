//! GUI presentation-core correctness against the demo torture files.
//!
//! Adversarial tests for the GUI defects: the silent open failure on
//! invalid-UTF-8 files, ligature formation through the shaping path, font
//! fallback across the configured stack, and grid/glyph alignment.
//! Markus Kuhn torture test must open byte-safe (never a silent empty
//! buffer), open failures must surface as typed errors, the configured font
//! stack must all be consulted by shaping (fallback), and shaped glyph
//! positions must agree with the grid the backgrounds paint on.

use erax::core::app::EditorApp;
use erax::gui::core::font_manager::{FontManager, FontSource};
use erax::gui::core::renderer::build_shaped_line;
use erax::terminal::display::{Cell, CellRole, Color as TermColor};
use std::path::PathBuf;
use std::sync::Arc;

fn demos(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("demos")
        .join(name)
}

/// Resolve one system font family to bytes, like the native shell does.
/// `None` when the family is not installed (the test then skips).
fn system_font(family: &str) -> Option<FontSource> {
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let id = db.query(&fontdb::Query {
        families: &[fontdb::Family::Name(family)],
        ..Default::default()
    })?;
    let (source, _) = db.face_source(id)?;
    let bytes: Vec<u8> = match source {
        fontdb::Source::Binary(data) => data.as_ref().as_ref().to_vec(),
        fontdb::Source::File(path) => std::fs::read(path).ok()?,
        fontdb::Source::SharedFile(path, ..) => std::fs::read(path).ok()?,
    };
    Some(FontSource {
        bytes: Arc::new(bytes),
        family: Some(family.to_string()),
    })
}

/// Shape `text` through a `FontManager` the way `Renderer::render` does
/// (whole row, `Shaping::Advanced`), returning layout glyph ids and x
/// positions.
fn shape_row(fm: &mut FontManager, text: &str) -> Vec<(u16, f32)> {
    use cosmic_text::{Align, Attrs, Buffer, Family, Metrics, Shaping};
    let cells: Vec<Cell> = text
        .chars()
        .map(|ch| Cell {
            ch,
            fg: TermColor::White,
            bg: TermColor::Reset,
            hidden: false,
            role: CellRole::EditorText,
        })
        .collect();
    let input = build_shaped_line(&cells);
    let mut buffer = Buffer::new(&mut fm.font_system, Metrics::new(16.0, 19.0));
    let family = fm.primary_family.clone();
    let mut attrs = Attrs::new();
    attrs = match family.as_deref() {
        Some(name) => attrs.family(Family::Name(name)),
        None => attrs.family(Family::Monospace),
    };
    buffer.set_text(&input.text, &attrs, Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(&mut fm.font_system, false);
    buffer
        .layout_runs()
        .flat_map(|run| {
            run.glyphs.iter().map(|g| {
                let physical = g.physical((0.0, 0.0), 1.0);
                (physical.cache_key.glyph_id, g.x)
            })
        })
        .collect()
}

/// The Markus Kuhn UTF-8 torture test carries invalid byte sequences by
/// design. Opening it must be byte-safe: the buffer opens (decoded view,
/// original bytes preserved for byte-exact save), never a silent empty
/// `[No Name]` buffer.
#[test]
fn torture_test_opens_byte_safe() {
    let path = demos("UTF-8-test.txt");
    assert!(path.exists(), "demo file must exist: {}", path.display());
    let expected_len = std::fs::metadata(&path).unwrap().len();

    let app = EditorApp::initialize(std::slice::from_ref(&path));
    let buffer = app.active_buffer().expect("an active buffer");
    let name = buffer
        .filename
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "[No Name]".to_string());
    assert!(
        name.ends_with("UTF-8-test.txt"),
        "torture test must open under its own name, got: {name}"
    );
    // The decoded view must carry the file's text (the torture sections are
    // ASCII-heavy; a byte-safe open never yields an empty buffer here).
    let text = buffer.content_string();
    assert!(
        text.len() > expected_len as usize / 2,
        "decoded view must not be empty/truncated: {} of {expected_len} bytes",
        text.len()
    );
    // Byte-exactness: the original bytes are preserved for save.
    assert!(
        buffer.original_bytes.as_ref().is_some_and(|b| b.len() == expected_len as usize),
        "original bytes must be preserved for byte-exact save"
    );
}

/// A file that cannot be opened must surface a typed error to the user —
/// never a silent empty buffer.
#[test]
fn open_failure_surfaces_typed_error() {
    let missing = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("demos")
        .join("does-not-exist-erax-test.txt");
    assert!(!missing.exists());

    let app = EditorApp::initialize(std::slice::from_ref(&missing));
    let message = app.message.as_deref().unwrap_or("");
    assert!(
        !message.is_empty(),
        "a failed open must leave a visible error message"
    );
    assert!(
        message.contains("does-not-exist-erax-test.txt"),
        "the error must name the file: {message}"
    );
    // The editor still has its empty buffer, but the failure is not silent.
    let buffer = app.active_buffer().expect("an active buffer");
    assert!(buffer.filename.is_none());
}

/// Valid UTF-8 files keep opening exactly as before (no behavior change on
/// the common path, no spurious messages).
#[test]
fn valid_utf8_open_is_unchanged() {
    let path = demos("UTF-8-demo.txt");
    let app = EditorApp::initialize(std::slice::from_ref(&path));
    let buffer = app.active_buffer().expect("an active buffer");
    assert!(
        buffer
            .filename
            .as_ref()
            .is_some_and(|p| p.ends_with("UTF-8-demo.txt")),
        "valid file must open under its own name"
    );
    assert!(
        buffer.original_bytes.is_none(),
        "clean UTF-8 keeps no preservation copy"
    );
    assert!(
        app.message.is_none(),
        "clean open leaves no message, got: {:?}",
        app.message
    );
}

/// The whole configured stack reaches the shaping database, primary first.
///
/// Loading only the first resolvable family leaves the configured fallback
/// dead: glyphs the primary lacks fall through to whatever the platform
/// picks (or tofu on the web, which has no system fonts at all).
#[test]
fn font_stack_loads_in_order() {
    let stack = erax::gui::shell::native::resolve_config_fonts();
    assert!(
        !stack.is_empty(),
        "at least one FONT_STACK family must resolve on a dev machine"
    );
    let fm = FontManager::from_font_stack(16.0, 19.0, &stack);
    assert_eq!(
        fm.primary_family.as_deref(),
        stack[0].family.as_deref(),
        "the stack head is the shaping family"
    );
    // Every stack entry shapes: none of them is dropped on the floor.
    for font in &stack {
        let mut fm = FontManager::from_font_stack(
            16.0,
            19.0,
            std::slice::from_ref(font),
        );
        let glyphs = shape_row(&mut fm, "M");
        assert_eq!(glyphs.len(), 1, "each stack font shapes M");
        assert_ne!(glyphs[0].0, 0, "M is not tofu");
    }
}

/// A glyph missing from the primary but present in a stack fallback shapes
/// from the fallback — not tofu, not an arbitrary system pick.
///
/// Finds the pair dynamically: U+0627 ARABIC LETTER ALEF is covered by
/// DejaVu Sans Mono but not by Noto Sans Mono on typical systems. Skips
/// when the system cannot supply such a pair.
#[test]
fn fallback_covers_glyph_missing_from_primary() {
    let primary = match system_font("Noto Sans Mono") {
        Some(f) => f,
        None => return,
    };
    let fallback = match system_font("DejaVu Sans Mono") {
        Some(f) => f,
        None => return,
    };
    // Confirm the premise: primary alone leaves tofu, fallback alone does not.
    let mut fm_primary = FontManager::from_font_stack(16.0, 19.0, std::slice::from_ref(&primary));
    let alone = shape_row(&mut fm_primary, "\u{627}");
    if alone.iter().all(|&(id, _)| id != 0) {
        return; // premise not met on this system; nothing to prove
    }
    let mut fm_fallback =
        FontManager::from_font_stack(16.0, 19.0, std::slice::from_ref(&fallback));
    let cover = shape_row(&mut fm_fallback, "\u{627}");
    if cover.iter().any(|&(id, _)| id == 0) {
        return; // premise not met on this system; nothing to prove
    }
    // The stack: the missing glyph resolves through the configured fallback.
    let stack = vec![primary, fallback];
    let mut fm = FontManager::from_font_stack(16.0, 19.0, &stack);
    let glyphs = shape_row(&mut fm, "\u{627}");
    assert!(
        glyphs.iter().all(|&(id, _)| id != 0),
        "stack fallback must cover the glyph the primary lacks (got tofu)"
    );
}

/// The shaping path forms ligatures when the font provides them.
///
/// DejaVu Sans Mono carries fi/fl under `dlig`; enabling it explicitly
/// through the same `Attrs` the renderer builds proves the path does not
/// suppress substitution. (Programming ligatures need a font whose `calt`
/// provides them — e.g. JetBrains Mono — plus this unobstructed path.)
#[test]
fn ligature_path_forms_ligatures() {
    use cosmic_text::{Align, Attrs, Buffer, Family, FeatureTag, FontFeatures, Metrics, Shaping};
    let font = match system_font("DejaVu Sans Mono") {
        Some(f) => f,
        None => return,
    };
    let mut fm = FontManager::from_font_stack(16.0, 19.0, std::slice::from_ref(&font));
    let mut features = FontFeatures::new();
    features.enable(FeatureTag::new(b"dlig"));
    let attrs = Attrs::new()
        .family(Family::Name("DejaVu Sans Mono"))
        .font_features(features);
    let mut buffer = Buffer::new(&mut fm.font_system, Metrics::new(16.0, 19.0));
    buffer.set_text("fi", &attrs, Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(&mut fm.font_system, false);
    let n: usize = buffer.layout_runs().map(|r| r.glyphs.len()).sum();
    assert_eq!(n, 1, "fi must shape as one ligature glyph, got {n}");
}

/// The grid cell width is the shaped advance, exactly — not rounded.
///
/// Background cells are located per fragment by `floor(pixel / cell)` and
/// shaped glyph x positions are multiples of the fractional advance, so an
/// integer-rounded cell width makes the grid drift from the glyphs one
/// fraction per column (the broken box-drawing alignment).
#[test]
fn grid_cell_size_is_the_exact_advance() {
    let font = match system_font("DejaVu Sans Mono") {
        Some(f) => f,
        None => return,
    };
    let fm = FontManager::from_font_stack(16.0, 19.0, std::slice::from_ref(&font));
    let (cw, ch) = fm.grid_cell_size();
    assert!(
        (cw - fm.cell_width).abs() < f32::EPSILON,
        "cell width must be the exact shaped advance, not rounded"
    );
    assert!(
        (ch - fm.cell_height).abs() < f32::EPSILON,
        "cell height must be exact"
    );
    assert!(
        cw.fract() != 0.0,
        "this test needs a fractional advance to be meaningful (got {cw})"
    );
}

/// Box-drawing glyphs land inside their grid cells.
///
/// Shapes the demo's box-drawing row through the real path and checks each
/// glyph's x against its column's cell: with the exact cell width the two
/// agree, with the old integer-rounded width they diverge visibly.
#[test]
fn box_drawing_glyphs_align_to_grid_cells() {
    let font = match system_font("DejaVu Sans Mono") {
        Some(f) => f,
        None => return,
    };
    let mut fm = FontManager::from_font_stack(16.0, 19.0, std::slice::from_ref(&font));
    let (cw, _) = fm.grid_cell_size();
    let row = "┌─────────┬─────────┐";
    let glyphs = shape_row(&mut fm, row);
    assert_eq!(
        glyphs.len(),
        row.chars().count(),
        "one glyph per box-drawing char"
    );
    for (i, &(_, x)) in glyphs.iter().enumerate() {
        let expected = i as f32 * cw;
        assert!(
            (x - expected).abs() < 1.0,
            "glyph {i} at x={x:.2} must sit in its cell (col {i} starts at {expected:.2}, cell width {cw:.3})"
        );
    }
}
