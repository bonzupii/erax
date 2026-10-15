//! Accessibility behavior settings: typed `pub const`s.
//!
//! Every accessibility feature in erax is configured here, as
//! compile-time constants like the rest of the configuration surface.
//! A personalized accessible setup is a small patch to this file, which
//! is also the delivery mechanism for the accessibility bootstrap
//! configs the owner provides directly to users who need them.

/// Master switch for accessibility cue events (mode-change earcons,
/// error bells, save/reload notices).
///
/// Off by default: a terminal bell or a screen flash firing without
/// the user asking for it is a regression for everyone else. Users
/// who want the cues set this to `true`.
pub const CUES_ENABLED: bool = false;

/// Ring the terminal bell (`\x07`) on error-class cues when
/// [`CUES_ENABLED`] is true. The bell never fires for mode changes or
/// informational cues; those are visual or silent.
pub const CUE_ERROR_BELL: bool = true;

/// Invert the active window's status bar for one frame on cue events
/// when [`CUES_ENABLED`] is true. The flash is carried as data through
/// the screen model (the renderer inverts the bar row while the flag
/// is set); it is the visual equivalent of the audio cues for users
/// who cannot hear them.
pub const CUE_VISUAL_FLASH: bool = true;

/// Announce the cursor's line number (not the column) to the web
/// shell's live region when it changes. Columns are never announced:
/// per-keystroke column chatter is the failure mode this disables.
pub const ANNOUNCE_CURSOR_LINE: bool = true;
