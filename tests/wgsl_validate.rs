//! WGSL validation for the live GUI shaders through naga.
//!
//! The renderer feeds these sources to wgpu; naga (pinned to the version
//! wgpu 30 validates with) parses and validates them here so invalid WGSL
//! fails the test suite instead of a GPU device. Covers exactly the
//! shaders the renderer `include_str!`s; the dead pre-split copies under
//! `src/gui/shaders/` are slated for deletion by the fix-audit and are
//! not validated.

/// The live shader set, as loaded by `src/gui/core/renderer.rs` and
/// `src/gui/core/quad_renderer.rs`.
const SHADERS: &[&str] = &[
    "src/gui/core/shaders/composite.wgsl",
    "src/gui/core/shaders/glyph.wgsl",
    "src/gui/core/shaders/cursor.wgsl",
];

#[test]
fn live_shaders_parse_and_validate() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for rel in SHADERS {
        let path = root.join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {rel}: {e}"));
        let module = naga::front::wgsl::parse_str(&src)
            .unwrap_or_else(|e| panic!("WGSL parse failed for {rel}: {e:?}"));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("WGSL validation failed for {rel}: {e:?}"));
    }
}
