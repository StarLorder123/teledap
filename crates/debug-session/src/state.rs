//! Session state machine types and transition logic.

use std::collections::HashSet;
use std::fmt;

/// The states a debug session can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SessionState {
    /// No debug adapter process is running.
    Disconnected,
    /// Debug adapter process is spawned but not yet initialized.
    Connected,
    /// initialize handshake is complete. Ready for launch/attach + configuration.
    Initialized,
    /// Debuggee is executing (running freely).
    Running,
    /// Debuggee is stopped at a breakpoint, step, exception, or pause.
    Halted,
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SessionState::Disconnected => write!(f, "Disconnected"),
            SessionState::Connected => write!(f, "Connected"),
            SessionState::Initialized => write!(f, "Initialized"),
            SessionState::Running => write!(f, "Running"),
            SessionState::Halted => write!(f, "Halted"),
        }
    }
}

impl SessionState {
    /// Returns whether `target` is a legal transition from `self`.
    pub fn can_transition_to(self, target: SessionState) -> bool {
        matches!(
            (self, target),
            (SessionState::Disconnected, SessionState::Connected)
                | (SessionState::Connected, SessionState::Initialized)
                | (SessionState::Connected, SessionState::Disconnected)
                | (SessionState::Initialized, SessionState::Running)
                | (SessionState::Initialized, SessionState::Disconnected)
                | (SessionState::Running, SessionState::Halted)
                | (SessionState::Running, SessionState::Disconnected)
                | (SessionState::Halted, SessionState::Running)
                | (SessionState::Halted, SessionState::Disconnected)
        )
    }

    /// List of states from which this state can be legally entered.
    pub fn valid_predecessors(self) -> &'static [SessionState] {
        match self {
            SessionState::Disconnected => &[
                SessionState::Connected,
                SessionState::Initialized,
                SessionState::Running,
                SessionState::Halted,
            ],
            SessionState::Connected => &[SessionState::Disconnected],
            SessionState::Initialized => &[SessionState::Connected],
            SessionState::Running => &[SessionState::Initialized, SessionState::Halted],
            SessionState::Halted => &[SessionState::Running],
        }
    }
}

/// Tracks which threads are currently halted and why.
#[derive(Debug, Clone, Default)]
pub struct HaltState {
    /// IDs of threads known to be stopped.
    pub stopped_threads: HashSet<u64>,
    /// Whether all threads are stopped (from all_threads_stopped).
    pub all_threads_stopped: bool,
    /// The reason for the most recent stop.
    pub last_stop_reason: Option<String>,
    /// IDs of the breakpoints that were hit (if any).
    pub hit_breakpoint_ids: Vec<u64>,
}

impl HaltState {
    /// Clear all halt state (called when execution resumes).
    pub fn clear(&mut self) {
        self.stopped_threads.clear();
        self.all_threads_stopped = false;
        self.last_stop_reason = None;
        self.hit_breakpoint_ids.clear();
    }

    /// Returns true if a specific thread is known to be stopped.
    pub fn is_thread_stopped(&self, thread_id: u64) -> bool {
        self.all_threads_stopped || self.stopped_threads.contains(&thread_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_legal_transitions() {
        // Disconnected -> Connected (start)
        assert!(SessionState::Disconnected.can_transition_to(SessionState::Connected));
        // Connected -> Initialized (initialize)
        assert!(SessionState::Connected.can_transition_to(SessionState::Initialized));
        // Connected -> Disconnected (shutdown before init)
        assert!(SessionState::Connected.can_transition_to(SessionState::Disconnected));
        // Initialized -> Running (launch + configurationDone)
        assert!(SessionState::Initialized.can_transition_to(SessionState::Running));
        // Initialized -> Disconnected (shutdown)
        assert!(SessionState::Initialized.can_transition_to(SessionState::Disconnected));
        // Running -> Halted (stopped event)
        assert!(SessionState::Running.can_transition_to(SessionState::Halted));
        // Running -> Disconnected (terminated event or shutdown)
        assert!(SessionState::Running.can_transition_to(SessionState::Disconnected));
        // Halted -> Running (continue/step)
        assert!(SessionState::Halted.can_transition_to(SessionState::Running));
        // Halted -> Disconnected (terminated event or shutdown)
        assert!(SessionState::Halted.can_transition_to(SessionState::Disconnected));
    }

    #[test]
    fn test_illegal_transitions() {
        // Cannot go backwards from Connected to Disconnected via normal operation
        // (Connected -> Disconnected is legal via shutdown, tested above)

        // Disconnected -> anything except Connected
        assert!(!SessionState::Disconnected.can_transition_to(SessionState::Initialized));
        assert!(!SessionState::Disconnected.can_transition_to(SessionState::Running));
        assert!(!SessionState::Disconnected.can_transition_to(SessionState::Halted));
        assert!(!SessionState::Disconnected.can_transition_to(SessionState::Disconnected));

        // Connected -> anything except Initialized or Disconnected
        assert!(!SessionState::Connected.can_transition_to(SessionState::Running));
        assert!(!SessionState::Connected.can_transition_to(SessionState::Halted));
        assert!(!SessionState::Connected.can_transition_to(SessionState::Connected));

        // Initialized -> anything except Running or Disconnected
        assert!(!SessionState::Initialized.can_transition_to(SessionState::Connected));
        assert!(!SessionState::Initialized.can_transition_to(SessionState::Halted));
        assert!(!SessionState::Initialized.can_transition_to(SessionState::Initialized));

        // Running -> anything except Halted or Disconnected
        assert!(!SessionState::Running.can_transition_to(SessionState::Connected));
        assert!(!SessionState::Running.can_transition_to(SessionState::Initialized));
        assert!(!SessionState::Running.can_transition_to(SessionState::Running));

        // Halted -> anything except Running or Disconnected
        assert!(!SessionState::Halted.can_transition_to(SessionState::Connected));
        assert!(!SessionState::Halted.can_transition_to(SessionState::Initialized));
        assert!(!SessionState::Halted.can_transition_to(SessionState::Halted));
    }

    #[test]
    fn test_valid_predecessors() {
        assert_eq!(
            SessionState::Disconnected.valid_predecessors(),
            &[
                SessionState::Connected,
                SessionState::Initialized,
                SessionState::Running,
                SessionState::Halted,
            ]
        );
        assert_eq!(
            SessionState::Connected.valid_predecessors(),
            &[SessionState::Disconnected]
        );
        assert_eq!(
            SessionState::Initialized.valid_predecessors(),
            &[SessionState::Connected]
        );
        assert_eq!(
            SessionState::Running.valid_predecessors(),
            &[SessionState::Initialized, SessionState::Halted]
        );
        assert_eq!(
            SessionState::Halted.valid_predecessors(),
            &[SessionState::Running]
        );
    }

    #[test]
    fn test_halt_state_clear() {
        let mut hs = HaltState {
            stopped_threads: [1, 2].into_iter().collect(),
            all_threads_stopped: true,
            last_stop_reason: Some("breakpoint".to_string()),
            hit_breakpoint_ids: vec![1],
        };
        hs.clear();
        assert!(hs.stopped_threads.is_empty());
        assert!(!hs.all_threads_stopped);
        assert!(hs.last_stop_reason.is_none());
        assert!(hs.hit_breakpoint_ids.is_empty());
    }

    #[test]
    fn test_is_thread_stopped() {
        let mut hs = HaltState::default();
        assert!(!hs.is_thread_stopped(1));

        hs.stopped_threads.insert(1);
        assert!(hs.is_thread_stopped(1));
        assert!(!hs.is_thread_stopped(2));

        hs.all_threads_stopped = true;
        assert!(hs.is_thread_stopped(2)); // all_threads_stopped covers all
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", SessionState::Disconnected), "Disconnected");
        assert_eq!(format!("{}", SessionState::Connected), "Connected");
        assert_eq!(format!("{}", SessionState::Initialized), "Initialized");
        assert_eq!(format!("{}", SessionState::Running), "Running");
        assert_eq!(format!("{}", SessionState::Halted), "Halted");
    }
}
