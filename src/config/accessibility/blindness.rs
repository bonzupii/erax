//! Non-visual operation settings: typed `pub const`s.
//!
//! Announcements for screen-reader live regions, audio cues that
//! replace visual signals, and the speech backend (voice input and
//! spoken output). All off or silent by default; enabling them never
//! changes what a sighted user sees.

use crate::speech::provider::{SpeechBackend, VoiceId};

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

/// Blindness profile master. When true, the accessibility cue bus
/// ([`super::CUES_ENABLED`]) goes live for the non-visual cue senses
/// in this module. This is the constant the `blind` accessibility
/// profile flips; it exists so profiles compose by class without
/// sharing patch ground.
pub const BLIND_PROFILE: bool = false;

/// Speech backend selection. Off by default: enabling it pulls
/// ~690 MiB of weights at build time and links the inference
/// runtimes.
pub const SPEECH_BACKEND: SpeechBackend = SpeechBackend::Off;

/// Voice for the built-in TTS.
pub const SPEECH_VOICE: VoiceId = VoiceId::AfHeart;
