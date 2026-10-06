//! The lifecycle state machine (ADR-0005). Pure: `apply` maps
//! `(state, command) → transition or typed rejection`. No clocks, no
//! processes — the actor layer feeds it and executes the transitions.

use crate::supervisor::{CrashPhase, LifecycleCommand, Transition};
use zamin_protocol::server::CrashClassification;

/// Mirror of the protocol state, kept local so the engine never depends on
/// the wire enum for its own invariants (conversion is one `From`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ServerState {
    NotRunning,
    Starting,
    Running,
    Stopping,
    Stopped,
    FailedPreflight,
    Crashed,
    Adopting,
    Unknown,
}

impl From<ServerState> for zamin_protocol::server::ServerState {
    fn from(state: ServerState) -> Self {
        use zamin_protocol::server::ServerState as Wire;
        match state {
            ServerState::NotRunning => Wire::NotRunning,
            ServerState::Starting => Wire::Starting,
            ServerState::Running => Wire::Running,
            ServerState::Stopping => Wire::Stopping,
            ServerState::Stopped => Wire::Stopped,
            ServerState::FailedPreflight => Wire::FailedPreflight,
            ServerState::Crashed => Wire::Crashed,
            ServerState::Adopting => Wire::Adopting,
            ServerState::Unknown => Wire::Unknown,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidTransition {
    pub state: ServerState,
    pub command: &'static str,
}

impl std::fmt::Display for InvalidTransition {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "command {} is not valid in state {}",
            self.command,
            state_name(self.state)
        )
    }
}

fn state_name(state: ServerState) -> &'static str {
    match state {
        ServerState::NotRunning => "not-running",
        ServerState::Starting => "starting",
        ServerState::Running => "running",
        ServerState::Stopping => "stopping",
        ServerState::Stopped => "stopped",
        ServerState::FailedPreflight => "failed-preflight",
        ServerState::Crashed => "crashed",
        ServerState::Adopting => "adopting",
        ServerState::Unknown => "unknown",
    }
}

/// The machine. One per server, owned by that server's actor.
#[derive(Debug, Clone)]
pub struct StateMachine {
    state: ServerState,
}

impl StateMachine {
    pub fn new(state: ServerState) -> Self {
        StateMachine { state }
    }

    pub fn state(&self) -> ServerState {
        self.state
    }

    /// Apply a command. The exit-code-carrying commands produce crash
    /// classifications; everything else is a plain transition.
    pub fn apply(&mut self, command: LifecycleCommand) -> Result<Transition, InvalidTransition> {
        use LifecycleCommand as Cmd;
        use ServerState as S;
        let command_name = command_name(&command);

        let transition = match (&self.state, &command) {
            // NotRunning: the only entry into `starting`.
            (S::NotRunning, Cmd::Start) => Some(Transition::To(S::Starting)),
            (S::Stopped, Cmd::Start) => Some(Transition::To(S::Starting)),
            (S::Crashed, Cmd::Start) => Some(Transition::To(S::Starting)),
            (S::FailedPreflight, Cmd::Start) => Some(Transition::To(S::Starting)),

            // Starting: the three outcomes.
            (S::Starting, Cmd::Spawned) => Some(Transition::To(S::Starting)),
            (S::Starting, Cmd::StartupValidated) => Some(Transition::To(S::Running)),
            (S::Starting, Cmd::StartupFailed { exit_code }) => {
                Some(Transition::CrashReported(CrashClassification {
                    phase: zamin_protocol::server::CrashPhase::from(CrashPhase::Startup),
                    exit_code: Some(*exit_code),
                    evidence: None,
                }))
            }
            (S::Starting, Cmd::PreflightFailed { .. }) => Some(Transition::To(S::FailedPreflight)),
            // A deliberate user stop during startup is a stop, not a crash:
            // unexpected exits before validation classify as startup
            // crashes, requested ones go through the normal shutdown path.
            (S::Starting, Cmd::Stop) => Some(Transition::To(S::Stopping)),

            // Running: stop or crash.
            (S::Running, Cmd::Stop) => Some(Transition::To(S::Stopping)),
            (S::Running, Cmd::Crashed { exit_code }) => {
                Some(Transition::CrashReported(CrashClassification {
                    phase: zamin_protocol::server::CrashPhase::from(CrashPhase::Runtime),
                    exit_code: Some(*exit_code),
                    evidence: None,
                }))
            }

            // Stopping: graceful completion or crash while shutting down.
            (S::Stopping, Cmd::StoppedGracefully) => Some(Transition::To(S::Stopped)),
            (S::Stopping, Cmd::Crashed { exit_code }) => {
                Some(Transition::CrashReported(CrashClassification {
                    phase: zamin_protocol::server::CrashPhase::from(CrashPhase::Shutdown),
                    exit_code: Some(*exit_code),
                    evidence: None,
                }))
            }

            // Adoption: verified identity proceeds; foreign identity is
            // never adopted, never killed (ADR-0005 hard invariant).
            (S::Adopting, Cmd::AdoptVerified) => Some(Transition::To(S::Running)),
            (S::Adopting, Cmd::AdoptForeign) => Some(Transition::To(S::Unknown)),

            // Reset: from any terminal state back to not-running (used by
            // registration removal and crash-card dismissal).
            (S::Stopped, Cmd::Reset)
            | (S::Crashed, Cmd::Reset)
            | (S::FailedPreflight, Cmd::Reset)
            | (S::Unknown, Cmd::Reset) => Some(Transition::To(S::NotRunning)),

            _ => None,
        };

        match transition {
            Some(Transition::To(next)) => {
                self.state = next;
                Ok(Transition::To(next))
            }
            Some(Transition::CrashReported(report)) => {
                self.state = ServerState::Crashed;
                Ok(Transition::CrashReported(report))
            }
            None => Err(InvalidTransition {
                state: self.state,
                command: command_name,
            }),
        }
    }
}

fn command_name(command: &LifecycleCommand) -> &'static str {
    use LifecycleCommand as Cmd;
    match command {
        Cmd::Start => "start",
        Cmd::PreflightFailed { .. } => "preflight-failed",
        Cmd::Spawned => "spawned",
        Cmd::StartupValidated => "startup-validated",
        Cmd::StartupFailed { .. } => "startup-failed",
        Cmd::Stop => "stop",
        Cmd::StoppedGracefully => "stopped-gracefully",
        Cmd::Crashed { .. } => "crashed",
        Cmd::AdoptVerified => "adopt-verified",
        Cmd::AdoptForeign => "adopt-foreign",
        Cmd::Reset => "reset",
    }
}
