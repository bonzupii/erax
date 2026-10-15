//! Web shell startup stage machine.
//!
//! The named stages of `erax_start`, in order. The web shell reports every
//! stage transition to the page status line and the console, and every
//! failure names its stage — startup never sits in an unnamed state. This
//! module is pure (no web_sys), so the transition graph is unit-testable on
//! the host.

/// One stage of web-shell startup, in definition order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupStage {
    /// Fetch the configured font from the server (`/font`).
    FetchFont,
    /// Build the presentation core: wgpu adapter, device, surface.
    CreateRenderer,
    /// Open the WebSocket to the server.
    ConnectSocket,
    /// Attach DOM listeners (keyboard, mouse, IME, resize, socket).
    WireEvents,
    /// Startup done; the client now waits for the server handshake.
    Running,
}

impl StartupStage {
    /// The first stage.
    pub fn first() -> StartupStage {
        StartupStage::FetchFont
    }

    /// Machine-readable stage name for the status line and console.
    pub fn name(self) -> &'static str {
        match self {
            StartupStage::FetchFont => "fetch-font",
            StartupStage::CreateRenderer => "create-renderer",
            StartupStage::ConnectSocket => "connect-socket",
            StartupStage::WireEvents => "wire-events",
            StartupStage::Running => "running",
        }
    }

    /// The stage entered after `self` succeeds; `None` past `Running`.
    pub fn next(self) -> Option<StartupStage> {
        match self {
            StartupStage::FetchFont => Some(StartupStage::CreateRenderer),
            StartupStage::CreateRenderer => Some(StartupStage::ConnectSocket),
            StartupStage::ConnectSocket => Some(StartupStage::WireEvents),
            StartupStage::WireEvents => Some(StartupStage::Running),
            StartupStage::Running => None,
        }
    }
}

/// One-line failure report naming the stage: shown on the page, logged to
/// the console, and used as the `erax_start` rejection reason.
pub fn startup_failure(stage: StartupStage, detail: &str) -> String {
    format!("startup failed in stage '{}': {detail}", stage.name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_order_is_total() {
        let mut stages = vec![StartupStage::first()];
        while let Some(next) = stages.last().unwrap().next() {
            stages.push(next);
        }
        assert_eq!(
            stages,
            [
                StartupStage::FetchFont,
                StartupStage::CreateRenderer,
                StartupStage::ConnectSocket,
                StartupStage::WireEvents,
                StartupStage::Running,
            ]
        );
    }

    #[test]
    fn running_is_terminal() {
        assert_eq!(StartupStage::Running.next(), None);
    }

    #[test]
    fn stage_names_are_distinct_and_nonempty() {
        let names = [
            StartupStage::FetchFont,
            StartupStage::CreateRenderer,
            StartupStage::ConnectSocket,
            StartupStage::WireEvents,
            StartupStage::Running,
        ]
        .map(|s| s.name());
        assert!(names.iter().all(|n| !n.is_empty()));
        let mut sorted = names.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), names.len());
    }

    #[test]
    fn failure_message_names_the_stage() {
        for stage in [
            StartupStage::FetchFont,
            StartupStage::CreateRenderer,
            StartupStage::ConnectSocket,
            StartupStage::WireEvents,
        ] {
            let msg = startup_failure(stage, "boom");
            assert!(msg.contains(stage.name()), "{msg}");
            assert!(msg.contains("boom"), "{msg}");
        }
    }
}
