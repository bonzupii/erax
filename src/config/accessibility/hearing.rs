//! Hearing-accessibility settings: typed `pub const`s.
//!
//! Visual equivalents of the audio cues, so every cue the editor
//! emits is perceivable without hearing. Silent by default like the
//! rest of the accessibility surface.

/// Invert the active window's status bar for one frame on cue events
/// when [`super::CUES_ENABLED`] is true. The flash is carried as data
/// through the screen model (the renderer inverts the bar row while
/// the flag is set); it is the visual equivalent of the audio cues
/// for users who cannot hear them.
pub const CUE_VISUAL_FLASH: bool = true;

/// Deafness profile master. When true, the accessibility cue bus
/// ([`super::CUES_ENABLED`]) goes live for the visual cue senses in
/// this module. This is the constant the `deaf` accessibility profile
/// flips; it exists so profiles compose by class without sharing
/// patch ground.
pub const DEAF_PROFILE: bool = false;
