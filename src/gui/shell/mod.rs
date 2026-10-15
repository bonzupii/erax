//! GUI shells: thin platform adapters around the presentation core.
//!
//! - `native`: the winit shell (windowing + raw input forwarding).
//! - `web`: the wasm shell (browser canvas + DOM input forwarding).
//!
//! Shells own no editor state, no input semantics, no rendering logic —
//! they construct the core (`crate::gui::core::Renderer`) and forward
//! events. The GUI stays a dumb passthrough (principle 12).

#[cfg(not(target_arch = "wasm32"))]
pub mod native;

#[cfg(target_arch = "wasm32")]
pub mod web;

/// Web-shell startup stage machine (pure logic; unit-tested on the host).
pub mod startup;
