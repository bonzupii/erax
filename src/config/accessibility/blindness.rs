//! Non-visual operation settings: typed `pub const`s.
//!
//! Announcements for screen-reader live regions, audio cues that
//! replace visual signals, and the speech backend (voice input and
//! spoken output). All off or silent by default; enabling them never
//! changes what a sighted user sees.

/// Announce the cursor's line number (not the column) to the web
/// shell's live region when it changes. Columns are never announced:
/// per-keystroke column chatter is the failure mode this disables.
pub const ANNOUNCE_CURSOR_LINE: bool = true;

/// Ring the terminal bell (`\x07`) on error-class cues when
/// [`super::CUES_ENABLED`] is true. The bell never fires for mode
/// changes or informational cues; those are visual or silent. This is
/// the non-visual error signal; its visual equivalent for deaf users
/// is [`super::hearing::CUE_VISUAL_FLASH`].
pub const CUE_ERROR_BELL: bool = true;
