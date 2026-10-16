//! Typed accessibility cue events and the bus that carries them.
//!
//! The core emits cues; shells map them to senses. A cue is a fact
//! about what happened (the mode changed, a save failed), never a
//! presentation decision: the TUI rings a bell, the web shell plays
//! an oscillator tone, and a future native sink will do whatever the
//! platform offers. The bus is the drop-in point: a new sink drains
//! the same queue.
//!
//! The bus is enabled by the compile-time
//! [`crate::config::accessibility::CUES_ENABLED`] constant. The enable
//! flag lives on the bus itself so tests exercise the gating logic
//! through the same seam the config feeds, without asserting the
//! constant's value.

use crate::core::bindings::ModalMode;
use std::collections::VecDeque;

/// One accessibility cue: a typed event the core emits for sinks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cue {
    /// The vim modal engine entered a different mode. Emitted exactly
    /// once per transition (entering the mode already held emits
    /// nothing); a silent mode change is a data-loss vector for
    /// non-visual users, so this is the safety-critical cue.
    ModeEntered(ModalMode),
    /// A user-visible error occurred (save failure, filter failure,
    /// calculator/sed/shell errors). The message itself travels the
    /// normal message path; the cue only marks the error class.
    Error,
    /// A buffer save completed successfully.
    SaveCompleted,
    /// The external-modification reload question was asked.
    ReloadQuestionAsked,
}

/// The cue queue. Owned by [`crate::core::app::EditorApp`]; each shell
/// drains it once per frame and maps the cues to its senses.
#[derive(Debug)]
pub struct CueBus {
    queue: VecDeque<Cue>,
    enabled: bool,
}

impl CueBus {
    /// Build a bus with an explicit enable flag. Production passes
    /// [`crate::config::accessibility::CUES_ENABLED`]; tests pass
    /// whatever the case under test needs.
    pub fn new(enabled: bool) -> Self {
        Self {
            queue: VecDeque::new(),
            enabled,
        }
    }

    /// Whether the bus delivers cues. A disabled bus drops every cue
    /// at emission: no sink ever sees them, and no queue grows.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Emit a cue. Dropped silently when the bus is disabled.
    pub fn emit(&mut self, cue: Cue) {
        if self.enabled {
            self.queue.push_back(cue);
        }
    }

    /// Number of cues waiting for a sink.
    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Drain every pending cue, oldest first. Sinks call this once per
    /// frame; anything not drained stays queued for the next drain.
    pub fn drain(&mut self) -> impl Iterator<Item = Cue> + '_ {
        self.queue.drain(..)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_bus_drops_everything() {
        let mut bus = CueBus::new(false);
        bus.emit(Cue::Error);
        bus.emit(Cue::ModeEntered(ModalMode::Insert));
        assert_eq!(bus.pending(), 0);
        assert_eq!(bus.drain().count(), 0);
    }

    #[test]
    fn enabled_bus_delivers_in_order() {
        let mut bus = CueBus::new(true);
        bus.emit(Cue::Error);
        bus.emit(Cue::ModeEntered(ModalMode::Insert));
        bus.emit(Cue::SaveCompleted);
        let got: Vec<Cue> = bus.drain().collect();
        assert_eq!(
            got,
            vec![
                Cue::Error,
                Cue::ModeEntered(ModalMode::Insert),
                Cue::SaveCompleted,
            ]
        );
        assert_eq!(bus.pending(), 0);
    }

    #[test]
    fn mode_cue_carries_the_new_mode() {
        let mut bus = CueBus::new(true);
        bus.emit(Cue::ModeEntered(ModalMode::Visual));
        assert_eq!(bus.drain().next(), Some(Cue::ModeEntered(ModalMode::Visual)));
    }
}
