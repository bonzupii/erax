//! The TUI sink for accessibility cue events.
//!
//! Once per frame the TUI loop drains the core's cue bus through
//! [`drain_cues_to_tui`]: error-class cues ring the terminal bell,
//! attention-class cues (mode changes, the reload question) set the
//! display's visual-flash flag, which the status renderer consumes as
//! data. The bell and flash policies arrive as parameters: production
//! passes the compile-time [`crate::config::accessibility`] constants,
//! tests pass whatever the case needs, so the gating logic is tested
//! through the same seam the config feeds.

use crate::core::app::EditorApp;
use crate::core::cue::Cue;
use crate::terminal::display::Display;
use std::io::Write;

/// Ring the terminal bell once. Best-effort: a bell that cannot be
/// written is not an error worth surfacing.
fn ring_bell() {
    let mut out = std::io::stdout();
    let _ = write!(out, "\x07");
    let _ = out.flush();
}

/// Drain the app's cue bus into the TUI's senses.
///
/// - [`Cue::Error`] rings the bell when `bell` is true.
/// - [`Cue::ModeEntered`] and [`Cue::ReloadQuestionAsked`] set the
///   display's flash flag (and mark it dirty so the frame renders)
///   when `flash` is true.
/// - [`Cue::SaveCompleted`] has no TUI manifestation: a bell or flash
///   per save would be noise, and the save is already visible where
///   the user looks: the status bar's modified flag clears. Audio
///   sinks still hear it.
///
/// A disabled bus delivers nothing, so the sink never fires when the
/// user left cues off: the gating lives in the bus, not here.
pub fn drain_cues_to_tui(app: &mut EditorApp, display: &mut Display, bell: bool, flash: bool) {
    for cue in app.cue_bus.drain() {
        match cue {
            Cue::Error => {
                if bell {
                    ring_bell();
                }
            }
            Cue::ModeEntered(_) | Cue::ReloadQuestionAsked => {
                if flash {
                    display.flash = true;
                    display.dirty = true;
                }
            }
            Cue::SaveCompleted => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::cue::CueBus;

    fn rig(enabled: bool) -> (EditorApp, Display) {
        let mut app = EditorApp::new();
        app.cue_bus = CueBus::new(enabled);
        let display = Display::new(80, 24, None, None);
        (app, display)
    }

    #[test]
    fn disabled_bus_fires_nothing() {
        let (mut app, mut display) = rig(false);
        // Even emitting directly cannot reach the sink: the bus drops.
        app.cue_bus.emit(Cue::Error);
        app.cue_bus.emit(Cue::ModeEntered(
            crate::config::keys::ModalMode::Insert,
        ));
        // Display::new starts dirty; the test cares about what the
        // sink changes, not the constructor.
        display.dirty = false;
        drain_cues_to_tui(&mut app, &mut display, true, true);
        assert!(!display.flash);
        assert!(!display.dirty);
    }

    #[test]
    fn error_cue_never_flashes() {
        let (mut app, mut display) = rig(true);
        app.cue_bus.emit(Cue::Error);
        display.dirty = false;
        // bell=false: the test asserts flash/dirty only, and a real
        // bell in the test output is noise.
        drain_cues_to_tui(&mut app, &mut display, false, true);
        assert!(!display.flash);
        assert!(!display.dirty);
    }

    #[test]
    fn mode_cue_flashes_and_dirties_when_enabled() {
        let (mut app, mut display) = rig(true);
        app.cue_bus.emit(Cue::ModeEntered(
            crate::config::keys::ModalMode::Insert,
        ));
        display.dirty = false;
        drain_cues_to_tui(&mut app, &mut display, true, true);
        assert!(display.flash);
        assert!(display.dirty);
    }

    #[test]
    fn flash_policy_off_leaves_display_alone() {
        let (mut app, mut display) = rig(true);
        app.cue_bus.emit(Cue::ModeEntered(
            crate::config::keys::ModalMode::Insert,
        ));
        display.dirty = false;
        drain_cues_to_tui(&mut app, &mut display, true, false);
        assert!(!display.flash);
        assert!(!display.dirty);
    }

    #[test]
    fn save_completed_is_silent_in_tui() {
        let (mut app, mut display) = rig(true);
        app.cue_bus.emit(Cue::SaveCompleted);
        display.dirty = false;
        drain_cues_to_tui(&mut app, &mut display, true, true);
        assert!(!display.flash);
        assert!(!display.dirty);
    }

    #[test]
    fn reload_question_flashes() {
        let (mut app, mut display) = rig(true);
        app.cue_bus.emit(Cue::ReloadQuestionAsked);
        display.dirty = false;
        drain_cues_to_tui(&mut app, &mut display, false, true);
        assert!(display.flash);
        assert!(display.dirty);
    }
}
