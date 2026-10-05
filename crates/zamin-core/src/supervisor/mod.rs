//! The server supervisor: per-server actors and the lifecycle state machine
//! (ADR-0005). The state machine is pure and table-driven — process wiring
//! joins it in the daemon's actors; transitions are the tested truth.

use zamin_protocol::server::CrashClassification;

pub mod state;

/// Lifecycle commands the actor accepts. Rejected commands are typed
/// errors, never silent no-ops.
#[derive(Debug, Clone, PartialEq)]
pub enum LifecycleCommand {
    Start,
    PreflightFailed {
        error: String,
    },
    Spawned,
    StartupValidated,
    StartupFailed {
        exit_code: i32,
    },
    Stop,
    /// Process exited while the machine was in `stopping`: the graceful
    /// path completed.
    StoppedGracefully,
    /// Unexpected exit: not stopping, not a startup validation window.
    Crashed {
        exit_code: i32,
    },
    AdoptVerified,
    AdoptForeign,
    Reset,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Transition {
    To(state::ServerState),
    CrashReported(CrashClassification),
}

/// Which phase a crash occurred in — drives the crash card (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPhase {
    Startup,
    Runtime,
    Shutdown,
}

impl From<CrashPhase> for zamin_protocol::server::CrashPhase {
    fn from(phase: CrashPhase) -> Self {
        match phase {
            CrashPhase::Startup => zamin_protocol::server::CrashPhase::Startup,
            CrashPhase::Runtime => zamin_protocol::server::CrashPhase::Runtime,
            CrashPhase::Shutdown => zamin_protocol::server::CrashPhase::Shutdown,
        }
    }
}

pub use state::ServerState;

#[cfg(test)]
mod tests;
