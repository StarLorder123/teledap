//! Debug session state machine.
//!
//! Defines the operational states of a debug session and validates
//! transitions between them.

/// Represents the operational state of the debugging session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// No connections established.
    Disconnected,
    /// OpenOCD connected, target powered but not debugged.
    Initialized,
    /// CodeLLDB attached to target via gdb-remote.
    Attached,
    /// Target is halted (breakpoint, step complete, reset halt).
    Halted,
    /// Target is running freely.
    Running,
}

impl SessionState {
    /// Returns the set of states that can be transitioned TO from the current state.
    pub fn allowed_transitions(&self) -> &[SessionState] {
        match self {
            SessionState::Disconnected => &[
                SessionState::Initialized,
                SessionState::Attached,
            ],
            SessionState::Initialized => &[
                SessionState::Attached,
                SessionState::Disconnected,
            ],
            SessionState::Attached => &[
                SessionState::Halted,
                SessionState::Running,
                SessionState::Initialized,
                SessionState::Disconnected,
            ],
            SessionState::Halted => &[
                SessionState::Running,
                SessionState::Attached,
                SessionState::Disconnected,
            ],
            SessionState::Running => &[
                SessionState::Halted,
                SessionState::Disconnected,
            ],
        }
    }

    /// Validates a transition. Returns Ok(()) or Err with description.
    pub fn validate_transition(&self, target: SessionState) -> Result<(), String> {
        if self.allowed_transitions().contains(&target) {
            Ok(())
        } else {
            Err(format!("Cannot transition from {:?} to {:?}", self, target))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_transitions() {
        assert!(SessionState::Disconnected
            .validate_transition(SessionState::Initialized)
            .is_ok());
        assert!(SessionState::Initialized
            .validate_transition(SessionState::Attached)
            .is_ok());
        assert!(SessionState::Attached
            .validate_transition(SessionState::Halted)
            .is_ok());
        assert!(SessionState::Halted
            .validate_transition(SessionState::Running)
            .is_ok());
        assert!(SessionState::Running
            .validate_transition(SessionState::Halted)
            .is_ok());
    }

    #[test]
    fn test_invalid_transitions() {
        // Cannot go from Disconnected to Halted (must attach first)
        assert!(SessionState::Disconnected
            .validate_transition(SessionState::Halted)
            .is_err());
        // Cannot go from Initialized to Halted (must attach first)
        assert!(SessionState::Initialized
            .validate_transition(SessionState::Halted)
            .is_err());
    }

    #[test]
    fn test_disconnected_to_attached() {
        // Disconnected -> Attached is valid for local debug mode
        assert!(SessionState::Disconnected
            .validate_transition(SessionState::Attached)
            .is_ok());
    }

    #[test]
    fn test_allowed_transitions_are_exhaustive() {
        // Verify every state has at least one valid transition
        let states = [
            SessionState::Disconnected,
            SessionState::Initialized,
            SessionState::Attached,
            SessionState::Halted,
            SessionState::Running,
        ];
        for state in &states {
            let allowed = state.allowed_transitions();
            assert!(
                !allowed.is_empty(),
                "State {:?} should have at least one allowed transition",
                state
            );
        }
    }
}
