//! Accessibility behavior settings, one module per disability class.
//!
//! Every accessibility feature in erax is configured here, as
//! compile-time constants like the rest of the configuration surface.
//! The classes with wired settings today are blindness (non-visual
//! operation: announcements, audio cues, speech) and hearing (visual
//! equivalents of audio cues). Vision, dyslexia, and motor modules
//! appear when their first wired constant lands; color-vision themes
//! stay in the themes module, not here. A personalized accessible
//! setup is a small patch to these files, which is also the delivery
//! mechanism for the preconfigured accessibility profiles.

pub mod blindness;
pub mod hearing;

/// Master switch for accessibility cue events (mode-change earcons,
/// error bells, save/reload notices).
///
/// Off by default: a terminal bell or a screen flash firing without
/// the user asking for it is a regression for everyone else. Users
/// who want the cues set this to `true`. The per-class modules hold
/// the individual cue senses; this constant gates the bus itself.
pub const CUES_ENABLED: bool = false;
